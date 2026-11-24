//! `group-length`: a length in a ce dimension group's scale, both ways.

use super::*;

/// `group-length` — read typed scaled lengths back to page user units
/// (`dimension::user_units_from_measurement_on`), and show page lengths in
/// the group's scale (`dimension::format_measurement_on`). Read-only.
///
/// ## Contract
///
/// - One header line `group-length <input> page=<N> group=<G> scale=<state>`,
///   then one line per `--typed` value,
///   `typed="<text>" user_units=<f64> readout="<text>"`, then one per
///   `--user-units` value, `user_units=<f64> readout="<text>"`. `readout`
///   is what the group shows for that length, so for `--typed` it is the
///   typed text again, to the format's precision.
/// - A `--typed` value that is not a length is reported on stderr and the
///   command exits `EDIT_REFUSED` after printing the rest; an unknown page
///   or group is refused before anything is printed.
pub(crate) fn cmd_group_length(
    input: &Path,
    page: usize,
    group: u32,
    typed: &[String],
    user_units: &[f64],
) -> u8 {
    use pdfcer_core::dimension::{GroupId, format_measurement_on, user_units_from_measurement_on};

    let doc = match open_for_read(input) {
        Ok(doc) => doc,
        Err(code) => return code,
    };
    let session = pdfcer_core::edit::EditSession::new(doc);
    let pages = match session.pages() {
        Ok(pages) => pages,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::EDIT_REFUSED;
        }
    };
    let Some(p) = page.checked_sub(1).and_then(|i| pages.get(i)) else {
        eprintln!(
            "pdfcer: {}: --page {page} is not a page (the document has {})",
            input.display(),
            pages.len()
        );
        return exit::EDIT_REFUSED;
    };
    let model = session.dimension_model();
    let Some(g) = model.group(GroupId(group)) else {
        eprintln!("pdfcer: {}: no dimension group {group}", input.display());
        return exit::EDIT_REFUSED;
    };
    let (scale, format) = (g.scale, g.format);
    println!(
        "group-length {} page={page} group={group} scale={scale:?}",
        input.display()
    );
    let mut code = exit::SUCCESS;
    for t in typed {
        match user_units_from_measurement_on(p, t, scale, format) {
            Ok(u) => println!(
                "typed={t:?} user_units={u} readout={:?}",
                format_measurement_on(p, u, scale, format).text
            ),
            Err(e) => {
                eprintln!("pdfcer: --typed {t:?}: {e}");
                code = exit::EDIT_REFUSED;
            }
        }
    }
    for &u in user_units {
        println!(
            "user_units={u} readout={:?}",
            format_measurement_on(p, u, scale, format).text
        );
    }
    code
}
