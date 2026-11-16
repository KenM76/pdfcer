//! `group-objects`, `ungroup-form`, `group-annotations` and
//! `ungroup-annotations`.

use super::*;
use pdfcer_core::edit::EditSession;

/// What every grouping subcommand takes besides its own selection.
pub(crate) struct GroupIo<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: u32,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

impl GroupIo<'_> {
    const fn page_index(&self) -> usize {
        (if self.page == 0 { 1 } else { self.page } - 1) as usize
    }

    /// Save, print `line` with the save report appended, and finish.
    fn save(&self, session: &mut EditSession, source: &[u8], line: &str) -> u8 {
        let outcome = match save_edited(
            session,
            source,
            self.output,
            self.mode,
            ProducerArg::Preserve,
            self.verify_undo,
        ) {
            Ok(outcome) => outcome,
            Err(code) => return code,
        };
        let r = &outcome.report;
        println!(
            "{line} mode={} -> {}; objects_written={} appended={} out_bytes={}",
            self.mode.name(),
            self.output.display(),
            r.objects_written,
            r.bytes_appended,
            r.bytes_written,
        );
        finish_edit(self.input, &outcome)
    }
}

/// `group-objects` — one undo-able grouping into a form XObject.
pub(crate) fn cmd_group_objects(io: &GroupIo, objects: &[usize]) -> u8 {
    let (source, mut session) = match open_for_edit(io.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let out = match session.group_objects(io.page_index(), objects) {
        Ok(out) => out,
        Err(err) => return report_edit_error(io.input, &err),
    };
    let line = format!(
        "group-objects {} page={} index={} form={} members={}",
        io.input.display(),
        io.page,
        out.index,
        out.form,
        out.members,
    );
    io.save(&mut session, &source, &line)
}

/// `ungroup-form` — one undo-able inlining of a form XObject.
pub(crate) fn cmd_ungroup_form(io: &GroupIo, object: usize) -> u8 {
    let (source, mut session) = match open_for_edit(io.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let out = match session.ungroup_form(io.page_index(), object) {
        Ok(out) => out,
        Err(err) => return report_edit_error(io.input, &err),
    };
    let line = format!(
        "ungroup-form {} page={} form={} indices={}..{} clipped={} renamed={}",
        io.input.display(),
        io.page,
        out.form,
        out.indices.start,
        out.indices.end,
        out.clipped,
        out.renamed,
    );
    io.save(&mut session, &source, &line)
}

/// `group-annotations` and, with no `members`, `ungroup-annotations`.
pub(crate) fn cmd_group_annotations(io: &GroupIo, index: usize, members: Option<&[usize]>) -> u8 {
    let (source, mut session) = match open_for_edit(io.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let page = io.page.max(1) as usize;
    let locate = |session: &EditSession, i: usize| {
        locate_annotation(session, io.input, (page, i), "group").map(|(id, _)| id)
    };
    let named = match locate(&session, index) {
        Ok(id) => id,
        Err(code) => return code,
    };
    let (verb, result) = match members {
        Some(members) => {
            let mut ids = Vec::with_capacity(members.len());
            for &m in members {
                match locate(&session, m) {
                    Ok(id) => ids.push(id),
                    Err(code) => return code,
                }
            }
            ("group-annotations", session.group_annotations(named, &ids))
        }
        None => ("ungroup-annotations", session.ungroup_annotations(named)),
    };
    let out = match result {
        Ok(out) => out,
        Err(err) => return report_edit_error(io.input, &err),
    };
    let ids: Vec<String> = out.members.iter().map(ToString::to_string).collect();
    let line = format!(
        "{verb} {} page={} primary={} members={} count={}",
        io.input.display(),
        io.page,
        out.primary,
        if ids.is_empty() {
            "none".to_owned()
        } else {
            ids.join(",")
        },
        ids.len(),
    );
    io.save(&mut session, &source, &line)
}
