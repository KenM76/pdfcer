use super::*;

/// Parse a `x,y x,y ...` (space/`;`-separated) point list into page-space
/// points. `None` on any malformed token or an empty list.
pub(crate) fn parse_dim_points(s: &str) -> Option<Vec<pdfcer_core::vector::Point>> {
    let mut out = Vec::new();
    for tok in s.split([' ', ';', '\t', '\n']).filter(|t| !t.is_empty()) {
        let (x, y) = tok.split_once(',')?;
        out.push(pdfcer_core::vector::Point::new(
            x.trim().parse().ok()?,
            y.trim().parse().ok()?,
        ));
    }
    (!out.is_empty()).then_some(out)
}

/// Parse an `N:M` ratio into `(paper, real)`. `None` if malformed.
pub(crate) fn parse_ratio(s: &str) -> Option<(f64, f64)> {
    let (a, b) = s.split_once(':')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

/// Borrowed argument bundle for [`cmd_dimension_add`] (clippy arg-count).
pub(crate) struct DimensionAddArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: u32,
    pub(crate) kind: DimKindArg,
    pub(crate) points: &'a str,
    pub(crate) group: u32,
    pub(crate) constraint: ConstraintArg,
    pub(crate) offset: f64,
    /// Force the parallel reading for `--kind two-lines` (operator override).
    pub(crate) treat_as_parallel: bool,
    pub(crate) text_along: f64,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `dimension-add` — author a scaled dimension additively (Pass 12.M2).
pub(crate) fn cmd_dimension_add(args: &DimensionAddArgs<'_>) -> u8 {
    use pdfcer_core::dimension::{DimensionKind, GroupId, fit_circle_taubin};
    let &DimensionAddArgs {
        input,
        page,
        kind,
        points,
        group,
        constraint,
        offset,
        treat_as_parallel,
        text_along,
        output,
        mode,
        verify_undo,
    } = args;

    // The operator's own near-parallel threshold. Read from the store rather
    // than defaulted here, so the CLI and the GUI slider cannot disagree
    // about when two lines count as parallel.
    let (settings, settings_report) =
        pdfcer_core::settings::Settings::load(pdfcer_core::settings::resolve_store());
    report_settings(&settings_report);

    let Some(pts) = parse_dim_points(points) else {
        eprintln!(
            "pdfcer: {}: --points must be `x,y x,y ...` in points",
            input.display()
        );
        return exit::EDIT_REFUSED;
    };
    let dk = match kind {
        DimKindArg::Linear => {
            let [a, b, ..] = pts.as_slice() else {
                eprintln!(
                    "pdfcer: {}: a linear dimension needs at least two points",
                    input.display()
                );
                return exit::EDIT_REFUSED;
            };
            DimensionKind::Linear {
                // Pass 27.1: the placement the operator asked for. Defaults to
                // 0.0/0.0 — the dimension line through the first picked point,
                // text centred — which is what the GUI's own neutral placement
                // produces, so the two surfaces still author identical bytes
                // for identical inputs.
                offset,
                text_along,
                a: *a,
                b: *b,
                constraint: constraint.to_core(),
            }
        }
        DimKindArg::Radius | DimKindArg::Diameter => {
            let Some(fit) = fit_circle_taubin(&pts) else {
                eprintln!(
                    "pdfcer: {}: need at least 3 non-collinear points to fit a circle",
                    input.display()
                );
                return exit::EDIT_REFUSED;
            };
            DimensionKind::Circular {
                fit,
                show_diameter: matches!(kind, DimKindArg::Diameter),
            }
        }
        // `Pass 107.0`. Two tokens for one kind, differing by the closing
        // segment — the same shape `radius`/`diameter` already has, and for
        // the same reason: a script filtering for fence runs and one filtering
        // for pipe runs are looking for different things.
        //
        // The minimums are pdfcer POLICY, not a spec requirement, and are
        // stated as such: ISO 32000-1 §12.5.6.9 sets no minimum vertex count
        // at all (its only count-adjacent sentence is permissive). The floor
        // comes from §12.9's distance function, which is defined for n >= 2,
        // plus the fact that a closed shape with two vertices traces a line
        // there and back — one stroke printing twice the distance between two
        // points.
        DimKindArg::Perimeter | DimKindArg::Path => {
            let closed = matches!(kind, DimKindArg::Perimeter);
            let minimum = if closed { 3 } else { 2 };
            if pts.len() < minimum {
                eprintln!(
                    "pdfcer: {}: --kind {} needs at least {minimum} points",
                    input.display(),
                    kind.token()
                );
                return exit::EDIT_REFUSED;
            }
            DimensionKind::Perimeter {
                points: pts.clone(),
                closed,
                // The placement pair, read in PAGE axes for this kind: the
                // label sits at the vertex centroid displaced by
                // (text_along, offset). See `DimensionKind::Perimeter::offset`
                // for why the centroid rather than the longest segment.
                offset,
                text_along,
            }
        }
        // The two-line mode: pdfcer reads the geometry and decides.
        //
        // The reading AND the authoring both live in
        // `pdfcer_core::dimension::two_lines`, shared verbatim with the GUI
        // gesture. This arm is now only argument-shaping and disclosure —
        // deliberately, because two copies of the sign convention and the arc
        // default is how the two shells come to author visibly different ce
        // dimensions from the same geometry.
        DimKindArg::TwoLines => {
            use pdfcer_core::dimension::{TwoLinePlacement, author_from_two_lines};
            use pdfcer_core::vector::linepick::{ParallelPolicy, PickedLine, TwoLineRelation};
            let [a1, a2, b1, b2, ..] = pts.as_slice() else {
                eprintln!(
                    "pdfcer: {}: --kind two-lines needs FOUR points — two for each \
                     line: `x,y x,y  x,y x,y`",
                    input.display()
                );
                return exit::EDIT_REFUSED;
            };
            // The pick point defaults to each segment's MIDPOINT.
            //
            // It is not decorative: two crossing lines bound four angles and
            // the pick chooses which one. The midpoint means "the angle on
            // the side the segments actually lie", which is the reading
            // someone typing coordinates intends. A GUI operator picks it by
            // clicking; there is no click here, so the choice is stated
            // rather than left implicit.
            let mk = |s: &pdfcer_core::vector::Point, e: &pdfcer_core::vector::Point| PickedLine {
                target: pdfcer_core::vector::HitTarget::Object(0),
                subpath: 0,
                segment: 0,
                start: *s,
                end: *e,
                pick: pdfcer_core::vector::Point::new(
                    f64::midpoint(s.x, e.x),
                    f64::midpoint(s.y, e.y),
                ),
            };
            let (la, lb) = (mk(a1, a2), mk(b1, b2));
            let mut policy = ParallelPolicy::from_setting(settings.parallel_epsilon_degrees);
            if treat_as_parallel {
                policy = policy.forcing_parallel();
            }
            let authored = match author_from_two_lines(
                &la,
                &lb,
                policy,
                TwoLinePlacement {
                    constraint: constraint.to_core(),
                    offset,
                    text_along,
                },
            ) {
                Ok(authored) => authored,
                Err(refusal) => {
                    // Both refusals are named, and the wording comes from the
                    // error type itself so the CLI and the GUI say the same
                    // thing about the same geometry.
                    eprintln!(
                        "pdfcer: {}: {refusal}. Nothing was authored.",
                        input.display()
                    );
                    return exit::EDIT_REFUSED;
                }
            };

            // Disclose the measurement AND the decision taken from it, always.
            // The operator gave four numbers; what pdfcer did with them is an
            // inference, and rule 4 says an inference is stated rather than
            // silently applied.
            if let Some(measured) = authored.measured_angle_degrees {
                println!(
                    "  two_lines measured_angle={measured:.3} epsilon={} forced={}",
                    settings.parallel_epsilon_degrees,
                    u32::from(treat_as_parallel)
                );
            }
            match authored.relation {
                // Refused above: `author_from_two_lines` returns
                // `TwoLineRefusal::Collinear` rather than an authoring, so this
                // arm is unreachable. Written as a no-op rather than a panic —
                // an impossible state is not worth aborting an edit over.
                TwoLineRelation::Collinear => {}
                TwoLineRelation::Parallel { distance } => {
                    println!("  two_lines authored=linear distance={distance:.4}");
                }
                TwoLineRelation::Angled {
                    degrees,
                    apex,
                    apex_is_real,
                } => {
                    println!(
                        "  two_lines authored=angular degrees={degrees:.3} \
                         apex={:.2},{:.2} apex_is_real={}",
                        apex.x,
                        apex.y,
                        u32::from(apex_is_real)
                    );
                    if !apex_is_real {
                        // Not a refusal — CAD drawings dimension a virtual
                        // apex routinely. But it is a fact about the drawing
                        // the operator may not have realised, so it is said.
                        eprintln!(
                            "pdfcer: {}: the two lines do not actually meet — the angle \
                             is measured at where they WOULD cross if extended.",
                            input.display()
                        );
                    }
                }
            }
            authored.kind
        }
    };

    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let Some(page_index) = page.checked_sub(1).map(|i| i as usize) else {
        eprintln!(
            "pdfcer: {}: --page is 1-based; 0 is not a page",
            input.display()
        );
        return exit::EDIT_REFUSED;
    };
    let (annot_id, dim_id) = match session.add_dimension(page_index, GroupId(group), dk) {
        Ok(v) => v,
        Err(err) => return report_edit_error(input, &err),
    };
    let outcome = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "dimension-add {} page {page} kind={} group={group} mode={} -> {}; \
annot={annot_id} dim={} changed={} objects={} verbatim={} appended={} out_bytes={} \
undo_verified={} undo_identical={}",
        input.display(),
        kind.token(),
        mode.name(),
        output.display(),
        dim_id.0,
        outcome.changed,
        r.objects_written,
        r.objects_verbatim,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(input, &outcome)
}

/// `--style-policy` for `format-text`: what pdfcer does when a bold/italic
/// request may need a fallback (`Pass 179.0`, decision 106).
///
/// A per-invocation override of the `style_policy` setting. Absent means "use
/// whatever is stored", which is the shape every other settings-backed flag in
/// this binary uses -- an `Option` rather than a `default_value_t`, so that
/// "not passed" and "passed the same value the setting holds" stay
/// distinguishable and passing nothing never overwrites a stored choice.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum StylePolicyArg {
    /// Decide and apply, silently. Reports which face was used afterwards.
    Auto,
    /// As auto, but say so loudly when the weight or slant was FAKED
    /// rather than real.
    Warn,
    /// Refuse an explicit fake-it request when a real face was available,
    /// and name that face. pdfcer's behaviour before this was a choice.
    Refuse,
}

impl StylePolicyArg {
    /// The core style policy this word names.
    pub(crate) const fn to_core(self) -> pdfcer_core::settings::StylePolicy {
        match self {
            Self::Auto => pdfcer_core::settings::StylePolicy::Auto,
            Self::Warn => pdfcer_core::settings::StylePolicy::Warn,
            Self::Refuse => pdfcer_core::settings::StylePolicy::Refuse,
        }
    }
}

/// `--members` for `group-delete`: what happens to the ce dimensions inside
/// a group being removed.
///
/// A **two-variant** mirror of `pdfcer_core::edit::GroupDeletion`, whose
/// `Reassign` arm carries its destination in the value. Clap cannot express a
/// value-carrying variant as a `--flag word`, so the destination rides on a
/// separate `--to` and the two are recombined in the handler — where a
/// `reassign` without a `--to`, and a `--to` without `reassign`, are both
/// refused by name rather than defaulted. Defaulting either one would pick a
/// destination group on the operator's behalf and re-measure every dimension
/// in it.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum GroupDeletionArg {
    /// Refuse if the group still has members, reporting how many.
    Refuse,
    /// Move the members to the group named by --to, re-measuring them.
    Reassign,
}

/// The drafting standard, as a CLI value.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum StandardArg {
    /// ANSI/ASME practice — broken dimension line, horizontal text, point
    /// decimal marker. pdfcer's default.
    Ansi,
    /// ISO 129-1 practice — unbroken line, value above and aligned, comma
    /// decimal marker.
    Iso,
}

impl StandardArg {
    /// The core drafting standard this word names.
    pub(crate) fn to_core(self) -> pdfcer_core::dimension::DimStandard {
        match self {
            Self::Ansi => pdfcer_core::dimension::DimStandard::Ansi,
            Self::Iso => pdfcer_core::dimension::DimStandard::Iso,
        }
    }
}

/// `group-rename` -- rename a ce dimension group (Pass 25.7).
///
/// ## Contract
///
/// - Emits one `group-rename ...` line carrying the OLD and the NEW name,
///   then defers the exit code to [`finish_edit`].
/// - **Both names**, because the id alone does not tell an operator reading a
///   log which group moved, and the old name is gone from the document the
///   moment this succeeds. It is also the fact a future paste depends on: a
///   ce dimension copied between documents is matched to a destination group
///   BY NAME, so a rename silently changes where a later paste lands.
/// - No appearance is regenerated -- a group's name is not drawn -- so there
///   is no member count to report and deliberately none is printed.
pub(crate) fn cmd_group_rename(
    input: &Path,
    group: u32,
    name: &str,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    // Read the old name BEFORE the rename, from the model rather than from an
    // argument -- the caller supplied only an id, and after the write the
    // previous name exists nowhere.
    let was = session
        .dimension_model()
        .group(pdfcer_core::dimension::GroupId(group))
        .map_or_else(String::new, |g| g.name.clone());
    if let Err(err) = session.rename_dimension_group(pdfcer_core::dimension::GroupId(group), name) {
        return report_edit_error(input, &err);
    }
    let outcome = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "group-rename {} group={group} was=\"{}\" now=\"{}\" mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        input.display(),
        was.replace('"', "'"),
        name.replace('"', "'"),
        mode.name(),
        output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(input, &outcome)
}

/// The `group-delete` argument bundle -- seven parameters, which is past the
/// point where positional arguments of the same type stop being readable
/// (`group` and `to` are both `u32` group ids and would sit side by side).
pub(crate) struct GroupDeleteArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) group: u32,
    pub(crate) members: GroupDeletionArg,
    pub(crate) to: Option<u32>,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `group-delete` -- remove a ce dimension group, saying what happens to its
/// members (Pass 25.7).
///
/// ## Contract
///
/// - Recombines `--members` and `--to` into the core `GroupDeletion` policy.
///   Both mismatches are refused BY NAME rather than defaulted -- `reassign`
///   without `--to`, and `--to` without `reassign`. A default destination
///   would pick a group on the operator's behalf and RE-MEASURE every
///   dimension moved into it, and a silently-ignored `--to` would let a
///   script believe it had specified a destination that was never read.
/// - Emits one `group-delete ...` line with `members_moved=`, then defers the
///   exit code to [`finish_edit`].
/// - A non-empty group under the default policy is refused through
///   [`report_edit_error`] with the engine's own sentence, which names the
///   member count.
pub(crate) fn cmd_group_delete(args: &GroupDeleteArgs) -> u8 {
    use pdfcer_core::edit::GroupDeletion;

    // Both halves of the policy are validated BEFORE the document is opened:
    // an argument error is not a document error, and reporting it against the
    // input path would suggest the file was at fault.
    let policy = match (args.members, args.to) {
        (GroupDeletionArg::Refuse, None) => GroupDeletion::Refuse,
        (GroupDeletionArg::Refuse, Some(_)) => {
            eprintln!(
                "pdfcer: --to names a destination group, which only `--members reassign` uses; pass `--members reassign` or drop `--to`"
            );
            return exit::EDIT_REFUSED;
        }
        (GroupDeletionArg::Reassign, Some(to)) => {
            GroupDeletion::Reassign(pdfcer_core::dimension::GroupId(to))
        }
        (GroupDeletionArg::Reassign, None) => {
            eprintln!(
                "pdfcer: --members reassign needs --to <GROUP>; the members would otherwise be moved to a group nobody chose, and re-measured against its scale"
            );
            return exit::EDIT_REFUSED;
        }
    };

    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let moved = match session
        .delete_dimension_group_with(pdfcer_core::dimension::GroupId(args.group), policy)
    {
        Ok(n) => n,
        Err(err) => return report_edit_error(args.input, &err),
    };
    let outcome = match save_edited(
        &mut session,
        &source,
        args.output,
        args.mode,
        ProducerArg::Preserve,
        args.verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "group-delete {} group={} members={} to={} members_moved={moved} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.group,
        match args.members {
            GroupDeletionArg::Refuse => "refuse",
            GroupDeletionArg::Reassign => "reassign",
        },
        args.to.map_or_else(|| "-".to_owned(), |t| t.to_string()),
        args.mode.name(),
        args.output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(args.input, &outcome)
}

/// `dimension-group` -- move one placed ce dimension into another group,
/// re-measuring it (Pass 25.7).
///
/// ## Contract
///
/// - Emits one `dimension-group ...` line carrying the printed value BEFORE
///   and AFTER the move, then defers the exit code to [`finish_edit`].
/// - **Both values, because the re-measurement IS the operation.** Scale,
///   unit, precision and drafting standard all live on the group, so a
///   re-parented ce dimension reads differently -- `5.000 m` becomes
///   `2.500 m` moving from a 1:50 group to a 1:100 one. That is correct and
///   is the single fact about this verb most likely to be reported as a
///   defect, so the CLI states it unasked. In the GUI the same disclosure
///   lives off-canvas; here the invocation is the commit, so the line is it.
/// - The values are read through `DimensionModel::display`, the one producer
///   the baked `/AP` also goes through, so the reported number cannot
///   disagree with what the page draws.
pub(crate) fn cmd_dimension_group(
    input: &Path,
    dimension: u32,
    group: u32,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let id = pdfcer_core::dimension::DimensionId(dimension);
    let before = session
        .dimension_model()
        .display(id)
        .map_or_else(String::new, |m| m.text);
    if let Err(err) = session.set_dimension_group(id, pdfcer_core::dimension::GroupId(group)) {
        return report_edit_error(input, &err);
    }
    let after = session
        .dimension_model()
        .display(id)
        .map_or_else(String::new, |m| m.text);
    let outcome = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "dimension-group {} dimension={dimension} group={group} was=\"{}\" now=\"{}\" remeasured={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        input.display(),
        before.replace('"', "'"),
        after.replace('"', "'"),
        u32::from(before != after),
        mode.name(),
        output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(input, &outcome)
}

/// `group-set-standard` — set a group's drafting standard, regenerating every
/// member (Pass 27.2).
///
/// ## Contract
///
/// - Emits one `group-set-standard …` line naming the group, the standard and
///   the MEMBER COUNT regenerated, then defers the exit code to
///   [`finish_edit`]. The count is reported because this changes the SHAPE of
///   every member, which is a larger visible change than a scale edit.
/// - An unknown group is refused before any mutation.
pub(crate) fn cmd_group_set_standard(
    input: &Path,
    group: u32,
    standard: StandardArg,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let members = match session
        .set_group_standard(pdfcer_core::dimension::GroupId(group), standard.to_core())
    {
        Ok(n) => n,
        Err(err) => return report_edit_error(input, &err),
    };
    let outcome = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "group-set-standard {} group={group} standard={} members_regenerated={members} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        input.display(),
        if matches!(standard, StandardArg::Iso) {
            "iso"
        } else {
            "ansi"
        },
        mode.name(),
        output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(input, &outcome)
}

/// Grouped arguments for `dimension-offset` (clippy arg-count).
pub(crate) struct DimensionOffsetArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) dimension: u32,
    pub(crate) offset: f64,
    pub(crate) text_along: f64,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// Which vertex edit `dimension-vertex` performs (`Pass 107.0`).
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub(crate) enum VertexOpArg {
    /// Move the vertex at --index by --dx/--dy. Re-measures.
    Move,
    /// Insert a new vertex at --at, immediately after --index.
    Insert,
    /// Remove the vertex at --index.
    Remove,
}

impl VertexOpArg {
    /// A stable token for CLI output.
    pub(crate) const fn token(self) -> &'static str {
        match self {
            VertexOpArg::Move => "move",     // ui-text-exempt: stable output token
            VertexOpArg::Insert => "insert", // ui-text-exempt: stable output token
            VertexOpArg::Remove => "remove", // ui-text-exempt: stable output token
        }
    }
}

/// Borrowed argument bundle for [`cmd_dimension_vertex`] (clippy arg-count).
/// Arguments of `annotation-vertex`, bundled so [`cmd_annotation_vertex`]
/// stays inside clippy's seven-argument limit.
pub(crate) struct AnnotationVertexArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: usize,
    pub(crate) annot: usize,
    pub(crate) op: VertexOpArg,
    pub(crate) index: usize,
    pub(crate) dx: f64,
    pub(crate) dy: f64,
    pub(crate) at: Option<&'a str>,
    pub(crate) modified: Option<&'a str>,
    pub(crate) dry_run: bool,
    pub(crate) output: Option<&'a Path>,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `annotation-vertex` (`Pass 255.0`): one vertex of a markup annotation,
/// moved / inserted / removed, through
/// `EditSession::reshape_annotation`.
///
/// The CLI has no session and no undo, so the invocation IS the commit and
/// every disclosure rides out with it (project rules 4 and 11): the vertex
/// count before and after, the recomputed `/Rect`, how the appearance
/// stream was written, anything the re-bake dropped, and — the one that
/// matters most because it is invisible — whether a `/Measure` the
/// annotation carries was left un-recomputed.
///
/// `--dry-run` goes through `reshape_annotation_preview`, which is the
/// same code up to the write, so a scripted preflight and the real thing
/// cannot disagree.
pub(crate) fn cmd_annotation_vertex(args: &AnnotationVertexArgs<'_>) -> u8 {
    use pdfcer_core::edit::{AppearanceWrite, VertexEdit};

    if args.page == 0 {
        eprintln!(
            "pdfcer: {}: --page is 1-based; 0 is not a page",
            args.input.display()
        );
        return exit::RUNTIME_ERROR;
    }
    let edit = match args.op {
        VertexOpArg::Move => VertexEdit::Move {
            index: args.index,
            dx: args.dx,
            dy: args.dy,
        },
        VertexOpArg::Insert => {
            let Some(text) = args.at else {
                eprintln!(
                    "pdfcer: {}: --op insert needs --at x,y (where the new vertex goes)",
                    args.input.display()
                );
                return exit::EDIT_REFUSED;
            };
            let Some(at) = parse_dim_points(text).and_then(|pts| pts.first().copied()) else {
                eprintln!(
                    "pdfcer: {}: --at must be `x,y` in points",
                    args.input.display()
                );
                return exit::EDIT_REFUSED;
            };
            VertexEdit::Insert {
                after: args.index,
                at,
            }
        }
        VertexOpArg::Remove => VertexEdit::Remove { index: args.index },
    };

    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    // Resolve (page, index) to an object id — the same addressing
    // `move-annotation` uses, so an operator lists once and edits many.
    let annot_id = {
        let slots = match session.page_slots() {
            Ok(slots) => slots,
            Err(err) => {
                eprintln!("pdfcer: {}: {err}", args.input.display());
                return exit::RUNTIME_ERROR;
            }
        };
        let Some(slot) = slots.get(args.page - 1) else {
            eprintln!(
                "pdfcer: {}: no page {} — the document has {} page(s)",
                args.input.display(),
                args.page,
                slots.len()
            );
            return exit::RUNTIME_ERROR;
        };
        let annots = pdfcer_core::annot::page_annotations(&session.graph(), slot.id);
        let Some(annot) = annots.get(args.annot) else {
            eprintln!(
                "pdfcer: {}: page {} has no annotation at index {} — it has {} (indices 0..{})",
                args.input.display(),
                args.page,
                args.annot,
                annots.len(),
                annots.len().saturating_sub(1)
            );
            return exit::RUNTIME_ERROR;
        };
        let Some(id) = annot.id else {
            eprintln!(
                "pdfcer: {}: page {} index {} is a direct dictionary inside /Annots, not an indirect object — it has no identity to reshape",
                args.input.display(),
                args.page,
                args.annot
            );
            return exit::EDIT_REFUSED;
        };
        id
    };

    let rect_token =
        |r: &pdfcer_core::page_tree::Rect| format!("{},{},{},{}", r.llx, r.lly, r.urx, r.ury);

    if args.dry_run {
        let f = match session.reshape_annotation_preview(annot_id, edit) {
            Ok(f) => f,
            Err(err) => return report_edit_error(args.input, &err),
        };
        if f.measure_not_recomputed {
            eprintln!(
                "pdfcer: {}: this annotation carries /Measure; its stated measurement would NOT be recomputed and may read stale after the reshape",
                args.input.display()
            );
        }
        println!(
            "annotation-vertex {} page={} annot={} subtype={} op={} index={} dry_run=1 vertices_before={} vertices_after={} rect_before={} rect_after={} measure_stale={}",
            args.input.display(),
            args.page,
            args.annot,
            sanitize_token(&f.subtype),
            f.edit.as_str(),
            args.index,
            f.vertices_before,
            f.vertices_after,
            f.rect_before
                .as_ref()
                .map_or_else(|| "none".to_owned(), rect_token),
            rect_token(&f.rect_after),
            u8::from(f.measure_not_recomputed),
        );
        return exit::SUCCESS;
    }

    let Some(output) = args.output else {
        eprintln!(
            "pdfcer: {}: --output is required unless --dry-run is passed",
            args.input.display()
        );
        return exit::EDIT_REFUSED;
    };

    let r = match session.reshape_annotation(annot_id, edit, args.modified) {
        Ok(r) => r,
        Err(err) => return report_edit_error(args.input, &err),
    };

    // Disclosures, invisible-first: the measurement caveat cannot be seen
    // on the page, the dropped properties can.
    if r.measure_not_recomputed {
        eprintln!(
            "pdfcer: {}: this annotation carries /Measure; its stated measurement was NOT recomputed — the geometry moved, the number did not, so it may now read stale",
            args.input.display()
        );
    }
    for d in &r.dropped {
        eprintln!(
            "pdfcer: {}: the regenerated appearance does not reproduce: {d:?}",
            args.input.display()
        );
    }

    let outcome = match save_edited(
        &mut session,
        &source,
        output,
        args.mode,
        ProducerArg::Preserve,
        args.verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let ap = match r.appearance {
        AppearanceWrite::InPlace(_) => "in-place",
        AppearanceWrite::Created(_) => "created",
        AppearanceWrite::CopiedOnWrite { .. } => "copied",
        // `AppearanceWrite` is #[non_exhaustive]; a future variant must
        // print SOMETHING rather than fail to compile a shell.
        _ => "other",
    };
    let rep = &outcome.report;
    println!(
        "annotation-vertex {} page={} annot={} subtype={} op={} index={} vertices_before={} vertices_after={} rect_before={} rect_after={} appearance={ap} dropped={} measure_stale={} m_written={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.page,
        args.annot,
        sanitize_token(&r.subtype),
        r.edit.as_str(),
        args.index,
        r.vertices_before,
        r.vertices_after,
        r.rect_before
            .as_ref()
            .map_or_else(|| "none".to_owned(), rect_token),
        rect_token(&r.rect_after),
        r.dropped.len(),
        u8::from(r.measure_not_recomputed),
        u8::from(r.mod_date_written),
        args.mode.name(),
        output.display(),
        outcome.changed,
        rep.objects_written,
        rep.bytes_appended,
        rep.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    exit::SUCCESS
}

/// Which ink edit `ink-edit` performs (`Pass 278.0`).
///
/// Six, not three, because an `/InkList` has two grains: a shell with a
/// 400-point stroke cannot offer per-point anchors and needs the stroke-level
/// verbs — the requesting project's own reasoning.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub(crate) enum InkOpArg {
    /// Move --point of --stroke by --dx/--dy.
    MovePoint,
    /// Insert a point at --at, immediately after --point.
    InsertPoint,
    /// Remove --point from --stroke. Floor: two points per stroke.
    RemovePoint,
    /// Replace --stroke's whole point list with --points.
    ReplaceStroke,
    /// Translate every point of --stroke by --dx/--dy.
    MoveStroke,
    /// Remove --stroke from the /InkList.
    RemoveStroke,
}

/// Arguments of `ink-edit`, bundled so [`cmd_ink_edit`] stays inside
/// clippy's seven-argument limit.
pub(crate) struct InkEditArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: usize,
    pub(crate) annot: usize,
    pub(crate) op: InkOpArg,
    pub(crate) stroke: usize,
    pub(crate) point: usize,
    pub(crate) dx: f64,
    pub(crate) dy: f64,
    pub(crate) at: Option<&'a str>,
    pub(crate) points: Option<&'a str>,
    pub(crate) modified: Option<&'a str>,
    pub(crate) dry_run: bool,
    pub(crate) output: Option<&'a Path>,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `ink-edit` (`Pass 278.0`): one point or one whole stroke of an `/Ink`
/// annotation, through `EditSession::reshape_ink`.
///
/// The CLI has no session and no undo, so the invocation IS the commit and
/// every disclosure rides out with it (project rules 4 and 11): the stroke
/// and point counts before and after, the recomputed `/Rect`, how the
/// appearance stream was written, anything the re-bake dropped, and — the
/// one that matters most because the operator cannot see it coming —
/// whether the artwork being replaced was pdfcer's own.
///
/// `--dry-run` goes through `reshape_ink_preview`, which is the same code up
/// to the write, so a scripted preflight and the real thing cannot disagree.
pub(crate) fn cmd_ink_edit(args: &InkEditArgs<'_>) -> u8 {
    use pdfcer_core::edit::{AppearanceWrite, InkEdit};

    if args.page == 0 {
        eprintln!(
            "pdfcer: {}: --page is 1-based; 0 is not a page",
            args.input.display()
        );
        return exit::RUNTIME_ERROR;
    }
    let edit = match args.op {
        InkOpArg::MovePoint => InkEdit::MovePoint {
            stroke: args.stroke,
            point: args.point,
            dx: args.dx,
            dy: args.dy,
        },
        InkOpArg::InsertPoint => {
            let Some(text) = args.at else {
                eprintln!(
                    "pdfcer: {}: --op insert-point needs --at x,y (where the new point goes)",
                    args.input.display()
                );
                return exit::EDIT_REFUSED;
            };
            let Some(at) = parse_dim_points(text).and_then(|pts| pts.first().copied()) else {
                eprintln!(
                    "pdfcer: {}: --at must be `x,y` in points",
                    args.input.display()
                );
                return exit::EDIT_REFUSED;
            };
            InkEdit::InsertPoint {
                stroke: args.stroke,
                after: args.point,
                at,
            }
        }
        InkOpArg::RemovePoint => InkEdit::RemovePoint {
            stroke: args.stroke,
            point: args.point,
        },
        InkOpArg::ReplaceStroke => {
            let Some(text) = args.points else {
                eprintln!(
                    "pdfcer: {}: --op replace-stroke needs --points x,y;x,y (at least two)",
                    args.input.display()
                );
                return exit::EDIT_REFUSED;
            };
            let Some(pts) = parse_dim_points(text) else {
                eprintln!(
                    "pdfcer: {}: --points must be `x,y;x,y;…` in points",
                    args.input.display()
                );
                return exit::EDIT_REFUSED;
            };
            InkEdit::ReplaceStroke {
                stroke: args.stroke,
                points: pts.iter().map(|p| (p.x, p.y)).collect(),
            }
        }
        InkOpArg::MoveStroke => InkEdit::MoveStroke {
            stroke: args.stroke,
            dx: args.dx,
            dy: args.dy,
        },
        InkOpArg::RemoveStroke => InkEdit::RemoveStroke {
            stroke: args.stroke,
        },
    };

    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    // Resolve (page, index) to an object id — the same addressing
    // `annotation-vertex` and `move-annotation` use, so an operator lists
    // once and edits many.
    let annot_id = {
        let slots = match session.page_slots() {
            Ok(slots) => slots,
            Err(err) => {
                eprintln!("pdfcer: {}: {err}", args.input.display());
                return exit::RUNTIME_ERROR;
            }
        };
        let Some(slot) = slots.get(args.page - 1) else {
            eprintln!(
                "pdfcer: {}: no page {} — the document has {} page(s)",
                args.input.display(),
                args.page,
                slots.len()
            );
            return exit::RUNTIME_ERROR;
        };
        let annots = pdfcer_core::annot::page_annotations(&session.graph(), slot.id);
        let Some(annot) = annots.get(args.annot) else {
            eprintln!(
                "pdfcer: {}: page {} has no annotation at index {} — it has {} (indices 0..{})",
                args.input.display(),
                args.page,
                args.annot,
                annots.len(),
                annots.len().saturating_sub(1)
            );
            return exit::RUNTIME_ERROR;
        };
        let Some(id) = annot.id else {
            eprintln!(
                "pdfcer: {}: page {} index {} is a direct dictionary inside /Annots, not an indirect object — it has no identity to reshape",
                args.input.display(),
                args.page,
                args.annot
            );
            return exit::EDIT_REFUSED;
        };
        id
    };

    let rect_token =
        |r: &pdfcer_core::page_tree::Rect| format!("{},{},{},{}", r.llx, r.lly, r.urx, r.ury);

    if args.dry_run {
        let f = match session.reshape_ink_preview(annot_id, &edit) {
            Ok(f) => f,
            Err(err) => return report_edit_error(args.input, &err),
        };
        if !f.appearance_was_pdfces {
            eprintln!(
                "pdfcer: {}: pdfcer did NOT draw this ink's appearance — the edit would REPLACE that artwork with pdfcer's own polyline rendering of the /InkList, which straightens a smoothed stroke. The geometry is moving, so carrying the old appearance is not an option.",
                args.input.display()
            );
        }
        println!(
            "ink-edit {} page={} annot={} op={} stroke={} dry_run=1 strokes_before={} strokes_after={} stroke_points_before={} stroke_points_after={} points_before={} points_after={} rect_before={} rect_after={} appearance_was_ours={}",
            args.input.display(),
            args.page,
            args.annot,
            f.edit.as_str(),
            f.stroke,
            f.strokes_before,
            f.strokes_after,
            f.stroke_points_before,
            f.stroke_points_after,
            f.points_before,
            f.points_after,
            f.rect_before
                .as_ref()
                .map_or_else(|| "none".to_owned(), rect_token),
            rect_token(&f.rect_after),
            u8::from(f.appearance_was_pdfces),
        );
        return exit::SUCCESS;
    }

    let Some(output) = args.output else {
        eprintln!(
            "pdfcer: {}: --output is required unless --dry-run is passed",
            args.input.display()
        );
        return exit::EDIT_REFUSED;
    };

    let r = match session.reshape_ink(annot_id, &edit, args.modified) {
        Ok(r) => r,
        Err(err) => return report_edit_error(args.input, &err),
    };

    // Disclosures, invisible-first: whose artwork this was cannot be seen in
    // the result, the dropped properties can.
    if !r.forecast.appearance_was_pdfces {
        eprintln!(
            "pdfcer: {}: pdfcer did NOT draw this ink's appearance — it has been REPLACED with pdfcer's own polyline rendering of the /InkList. A smoothed stroke is now straight between its points.",
            args.input.display()
        );
    }
    for d in &r.dropped {
        eprintln!(
            "pdfcer: {}: the regenerated appearance does not reproduce: {d:?}",
            args.input.display()
        );
    }

    let outcome = match save_edited(
        &mut session,
        &source,
        output,
        args.mode,
        ProducerArg::Preserve,
        args.verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let ap = match r.appearance {
        AppearanceWrite::InPlace(_) => "in-place",
        AppearanceWrite::Created(_) => "created",
        AppearanceWrite::CopiedOnWrite { .. } => "copied",
        // `AppearanceWrite` is #[non_exhaustive]; a future variant must print
        // SOMETHING rather than fail to compile a shell.
        _ => "other",
    };
    let f = &r.forecast;
    let rep = &outcome.report;
    println!(
        "ink-edit {} page={} annot={} op={} stroke={} strokes_before={} strokes_after={} stroke_points_before={} stroke_points_after={} points_before={} points_after={} rect_before={} rect_after={} appearance={ap} appearance_was_ours={} dropped={} m_written={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.page,
        args.annot,
        f.edit.as_str(),
        f.stroke,
        f.strokes_before,
        f.strokes_after,
        f.stroke_points_before,
        f.stroke_points_after,
        f.points_before,
        f.points_after,
        f.rect_before
            .as_ref()
            .map_or_else(|| "none".to_owned(), rect_token),
        rect_token(&f.rect_after),
        u8::from(f.appearance_was_pdfces),
        r.dropped.len(),
        u8::from(r.mod_date_written),
        args.mode.name(),
        output.display(),
        outcome.changed,
        rep.objects_written,
        rep.bytes_appended,
        rep.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    exit::SUCCESS
}

pub(crate) struct DimensionVertexArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) dimension: u32,
    pub(crate) op: VertexOpArg,
    pub(crate) index: usize,
    pub(crate) dx: f64,
    pub(crate) dy: f64,
    pub(crate) at: Option<&'a str>,
    pub(crate) dry_run: bool,
    pub(crate) output: Option<&'a Path>,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `dimension-vertex` — the CLI half of `Pass 107.1`'s vertex editing.
///
/// # The disclosure is not optional here, and the CLI's form of it is print
///
/// A vertex edit re-measures. In the GUI the operator watches the number
/// change and Save is the commit point (decision 059); the CLI has no session,
/// so the invocation is the commit and there is nothing to watch. Rule 11's
/// answer is that the CLI PRINTS what it inferred on the way past, which is
/// why `before=` and `after=` are on the output line rather than only the
/// resulting value.
///
/// # `--dry-run` is the preflight, not a separate code path
///
/// It calls `vertex_edit_preview`, which shares one body with the mutating
/// verb — so a dry run that says "this will work" and a real run that then
/// refuses is not a state this command can reach.
pub(crate) fn cmd_dimension_vertex(args: &DimensionVertexArgs<'_>) -> u8 {
    use pdfcer_core::dimension::DimensionId;
    use pdfcer_core::edit::VertexEdit;

    let id = DimensionId(args.dimension);
    let edit = match args.op {
        VertexOpArg::Move => VertexEdit::Move {
            index: args.index,
            dx: args.dx,
            dy: args.dy,
        },
        VertexOpArg::Insert => {
            let Some(text) = args.at else {
                eprintln!(
                    "pdfcer: {}: --op insert needs --at x,y (where the new vertex goes)",
                    args.input.display()
                );
                return exit::EDIT_REFUSED;
            };
            let Some(pts) = parse_dim_points(text) else {
                eprintln!(
                    "pdfcer: {}: --at must be `x,y` in points",
                    args.input.display()
                );
                return exit::EDIT_REFUSED;
            };
            let Some(at) = pts.first().copied() else {
                eprintln!(
                    "pdfcer: {}: --at must be `x,y` in points",
                    args.input.display()
                );
                return exit::EDIT_REFUSED;
            };
            VertexEdit::Insert {
                after: args.index,
                at,
            }
        }
        VertexOpArg::Remove => VertexEdit::Remove { index: args.index },
    };

    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    if args.dry_run {
        let forecast = match session.vertex_edit_preview(id, edit) {
            Ok(o) => o,
            Err(err) => return report_edit_error(args.input, &err),
        };
        println!(
            "dimension-vertex {} dimension={} op={} index={} dry_run=1 vertices={} closed={} before={:?} after={:?}",
            args.input.display(),
            args.dimension,
            args.op.token(),
            args.index,
            forecast.vertices,
            u32::from(forecast.closed),
            forecast.previous_label,
            forecast.label,
        );
        return exit::SUCCESS;
    }

    let Some(output) = args.output else {
        eprintln!(
            "pdfcer: {}: --output is required unless --dry-run is passed",
            args.input.display()
        );
        return exit::EDIT_REFUSED;
    };

    let applied = match args.op {
        VertexOpArg::Move => session.move_dimension_vertex(id, args.index, args.dx, args.dy),
        VertexOpArg::Insert => match edit {
            VertexEdit::Insert { after, at } => session.insert_dimension_vertex(id, after, at),
            // Unreachable: `edit` was built from `args.op` immediately above.
            _ => unreachable!("op and edit are built together"),
        },
        VertexOpArg::Remove => session.remove_dimension_vertex(id, args.index),
    };
    let applied = match applied {
        Ok(o) => o,
        Err(err) => return report_edit_error(args.input, &err),
    };

    let outcome = match save_edited(
        &mut session,
        &source,
        output,
        args.mode,
        ProducerArg::Preserve,
        args.verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "dimension-vertex {} dimension={} op={} index={} vertices={} closed={} before={:?} after={:?} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.dimension,
        args.op.token(),
        args.index,
        applied.vertices,
        u32::from(applied.closed),
        applied.previous_label,
        applied.label,
        args.mode.name(),
        output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(args.input, &outcome)
}

/// `dimension-rotate` — turn one ce dimension about a point (`Pass 159.0`).
///
/// Prints the constraint relaxation when it happens, because a dimension that
/// silently stopped being locked to horizontal is a drafting surprise the
/// operator would find later and blame on something else.
pub(crate) fn cmd_dimension_rotate(
    input: &Path,
    dimension: u32,
    degrees: f64,
    pivot: (f64, f64),
    output: &Path,
    mode: SaveMode,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let id = pdfcer_core::dimension::DimensionId(dimension);

    let before = session
        .dimension_model()
        .display(id)
        .map(|d| d.text)
        .unwrap_or_default();

    let out = match session.rotate_dimension(id, pivot, degrees) {
        Ok(o) => o,
        Err(err) => return report_edit_error(input, &err),
    };

    if out.constraint_relaxed {
        eprintln!(
            "pdfcer: {}: this ce dimension was locked to an axis, and the rotation RELAXED that lock to \"aligned\". A horizontal or vertical constraint cannot describe a line that has been turned, and keeping it would leave the drawn line disagreeing with its own stated constraint.",
            input.display()
        );
    }

    let after = session
        .dimension_model()
        .display(id)
        .map(|d| d.text)
        .unwrap_or_default();
    if after != before {
        eprintln!(
            "pdfcer: {}: the measured value CHANGED, {before:?} -> {after:?}, and it should not have. A rotation preserves every distance, so this is a defect rather than an expected outcome -- please report it.",
            input.display()
        );
    }

    let saved = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        false,
    ) {
        Ok(o) => o,
        Err(code) => return code,
    };

    println!(
        "dimension-rotate {} dimension={dimension} degrees={:.4} pivot=({:.2} {:.2}) -> {}",
        input.display(),
        out.degrees,
        pivot.0,
        pivot.1,
        output.display()
    );
    println!(
        "  value={after:?} (unchanged, by construction) constraint_relaxed={}",
        out.constraint_relaxed
    );
    finish_edit(input, &saved)
}

/// `dimension-offset` — set a ce dimension's placement (Pass 27.1).
///
/// ## Contract
///
/// - Emits one `dimension-offset …` line with the usual save-report fields,
///   then defers the exit code to [`finish_edit`].
/// - The measured value is unchanged by construction: this writes only the
///   placement fields, which the value function does not read.
/// - A circular target, or an unknown id, is refused through
///   [`report_edit_error`] before any mutation — the same message and exit
///   code the GUI surfaces.
pub(crate) fn cmd_dimension_offset(args: &DimensionOffsetArgs<'_>) -> u8 {
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    if let Err(err) = session.place_dimension(
        pdfcer_core::dimension::DimensionId(args.dimension),
        args.offset,
        args.text_along,
    ) {
        return report_edit_error(args.input, &err);
    }
    let outcome = match save_edited(
        &mut session,
        &source,
        args.output,
        args.mode,
        ProducerArg::Preserve,
        args.verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "dimension-offset {} dimension={} offset={} text_along={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.dimension,
        args.offset,
        args.text_along,
        args.mode.name(),
        args.output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(args.input, &outcome)
}

/// `dimension-display` — switch a placed circular ce dimension between the
/// radius and the diameter reading (Pass 34.2).
///
/// ## Contract
///
/// - Emits one `dimension-display …` line with the usual save-report fields,
///   then defers the exit code to [`finish_edit`].
/// - An unknown id, **or a LINEAR ce dimension**, is refused through
///   [`report_edit_error`] before any mutation, with the same message and exit
///   code the GUI surfaces. The linear refusal is the interesting one: it is
///   how a script learns it aimed the verb at the wrong ce dimension rather
///   than writing a file in which nothing changed.
/// - Six parameters rather than a borrowed args struct: the sibling
///   `dimension-offset` needed one to stay under clippy's arg-count ceiling
///   because it carries two extra `f64`s; this one has the same shape as
///   `dimension-delete`, which takes them plainly.
pub(crate) fn cmd_dimension_display(
    input: &Path,
    dimension: u32,
    show: DisplayReading,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    if let Err(err) = session.set_dimension_display(
        pdfcer_core::dimension::DimensionId(dimension),
        show.show_diameter(),
    ) {
        return report_edit_error(input, &err);
    }
    let outcome = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "dimension-display {} dimension={dimension} show={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        input.display(),
        show.token(),
        mode.name(),
        output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(input, &outcome)
}

/// `dimension-label` -- set or clear one ce dimension's text override
/// (Pass 175.0, decision 097).
///
/// ## Contract
///
/// - `label=None` (from `--clear`) clears the override; `Some(text)` sets it.
/// - Emits one `dimension-label ...` line carrying BOTH captions --
///   `measured=` (what the geometry says) and `printed=` (what the page now
///   draws) -- plus `changed=`, then defers the exit code to [`finish_edit`].
/// - **Both captions, always.** In the GUI the disclosure lives off-canvas in
///   a panel; in the CLI the invocation IS the commit, so the disclosure is
///   this line (`CLAUDE.md` rule 4's CLI half: the CLI prints what it
///   inferred on the way past). An operator who overrides a dimension to read
///   something other than its measurement is entitled to see, in the same
///   breath, what the measurement was.
/// - `changed=0` means the requested state already held; nothing was
///   committed and no undo entry was pushed, so the save is a no-op.
/// - A refusal (empty, over-long, or unprintable text; unknown id) goes
///   through [`report_edit_error`] before any mutation, with the same message
///   and exit code the GUI surfaces.
pub(crate) fn cmd_dimension_label(
    input: &Path,
    dimension: u32,
    label: Option<&str>,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let change =
        match session.set_dimension_label(pdfcer_core::dimension::DimensionId(dimension), label) {
            Ok(change) => change,
            Err(err) => return report_edit_error(input, &err),
        };
    let outcome = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "dimension-label {} dimension={dimension} overridden={} changed={} measured=\"{}\" printed=\"{}\" mode={} -> {}; changed_objects={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        input.display(),
        u32::from(change.applied.is_some()),
        u32::from(change.changed),
        change.measured.replace('"', "'"),
        change.printed.replace('"', "'"),
        mode.name(),
        output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(input, &outcome)
}

/// `dimension-delete` — remove one ce dimension and every trace of it.
///
/// ## Contract
///
/// - Emits one `dimension-delete …` line with the usual save-report fields,
///   then defers the exit code to [`finish_edit`].
/// - An unknown id is refused through [`report_edit_error`] before any
///   mutation, with the same message and exit code the GUI surfaces.
pub(crate) fn cmd_dimension_delete(
    input: &Path,
    dimension: u32,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    if let Err(err) = session.delete_dimension(pdfcer_core::dimension::DimensionId(dimension)) {
        return report_edit_error(input, &err);
    }
    let outcome = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "dimension-delete {} dimension={dimension} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        input.display(),
        mode.name(),
        output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(input, &outcome)
}

/// The style properties `--clear` can name, and the vocabulary
/// `dimension-list --style` prints.
///
/// One enum shared by both commands and both directions (setting and
/// reporting), so a property cannot be clearable under one name and reported
/// under another.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum StylePropArg {
    /// Display unit (ce-dimension tier only).
    Unit,
    /// Decimal places / fraction denominator (ce-dimension tier only).
    Fraction,
    /// Decimal marker (ce-dimension tier only).
    DecimalMarker,
    /// Drafting standard (ce-dimension tier only).
    Standard,
    /// Label point size.
    TextHeight,
    /// Stroke width.
    LineWidth,
    /// Arrowhead length.
    ArrowLength,
    /// Terminator form.
    ArrowForm,
    /// Colour.
    Color,
    /// Tolerance.
    Tolerance,
    /// Tolerance precision.
    TolerancePlaces,
}

/// The terminator forms, mirrored from `pdfcer_core::dimension::ArrowForm`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum ArrowFormArg {
    /// Solid filled triangle - ANSI/ASME mechanical practice, the default.
    Filled,
    /// Open (stroked) V - common in architectural work.
    Open,
    /// 45-degree tick through the dimension line - architectural practice.
    Slash,
    /// Filled dot.
    Dot,
    /// No terminator.
    None,
}

impl ArrowFormArg {
    /// The core arrowhead form this word names.
    pub(crate) fn to_core(self) -> pdfcer_core::dimension::ArrowForm {
        use pdfcer_core::dimension::ArrowForm as F;
        match self {
            Self::Filled => F::Filled,
            Self::Open => F::Open,
            Self::Slash => F::Slash,
            Self::Dot => F::Dot,
            Self::None => F::None,
        }
    }
}

/// The decimal marker, mirrored from `pdfcer_core::dimension::DecimalMarker`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum DecimalMarkerArg {
    /// 1.5 - ANSI/ASME practice.
    Point,
    /// 1,5 - mandated by ISO 129-1:2018 cl. 4.1.1.
    Comma,
}

impl DecimalMarkerArg {
    /// The core decimal marker this word names.
    pub(crate) fn to_core(self) -> pdfcer_core::dimension::DecimalMarker {
        match self {
            Self::Point => pdfcer_core::dimension::DecimalMarker::Point,
            Self::Comma => pdfcer_core::dimension::DecimalMarker::Comma,
        }
    }
}

/// The five appearance flags both style commands share.
pub(crate) struct AppearanceArgs {
    pub(crate) text_height: Option<f64>,
    pub(crate) line_width: Option<f64>,
    pub(crate) arrow_length: Option<f64>,
    pub(crate) arrow_form: Option<ArrowFormArg>,
    pub(crate) color: Option<String>,
    pub(crate) tolerance: Option<String>,
    pub(crate) tolerance_places: Option<u32>,
}

pub(crate) struct GroupStyleArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) group: u32,
    pub(crate) appearance: AppearanceArgs,
    pub(crate) clear: &'a [StylePropArg],
    pub(crate) reset: bool,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

pub(crate) struct DimensionStyleArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) dimension: u32,
    pub(crate) unit: Option<&'a str>,
    pub(crate) places: Option<u32>,
    pub(crate) denominator: Option<u32>,
    pub(crate) reduce: bool,
    pub(crate) decimal_marker: Option<DecimalMarkerArg>,
    pub(crate) standard: Option<StandardArg>,
    pub(crate) appearance: AppearanceArgs,
    pub(crate) clear: &'a [StylePropArg],
    pub(crate) reset: bool,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// Parse `--color`: either `r,g,b` components in 0.0-1.0, or `#rrggbb`.
///
/// # Why two forms
///
/// `r,g,b` is what the PDF object model actually stores (`/C` is an array of
/// three numbers in 0.0-1.0), so it is the honest primary form. `#rrggbb` is
/// what an operator has in his hand from anywhere else. Refusing the hex form
/// would make the flag technically correct and practically annoying; accepting
/// only hex would hide the storage.
pub(crate) fn parse_style_color(spec: &str) -> Result<pdfcer_core::vector::Rgb, String> {
    let s = spec.trim();
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!("`{spec}` is not a #rrggbb colour"));
        }
        let byte = |i: usize| -> f32 {
            f32::from(u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0)) / 255.0
        };
        return Ok(pdfcer_core::vector::Rgb {
            r: byte(0),
            g: byte(2),
            b: byte(4),
        });
    }
    let parts: Vec<&str> = s.split(',').map(str::trim).collect();
    if parts.len() != 3 {
        return Err(format!(
            "`{spec}` is not a colour: expected `r,g,b` in 0.0-1.0, or `#rrggbb`"
        ));
    }
    let mut c = [0.0f32; 3];
    for (slot, text) in c.iter_mut().zip(&parts) {
        let v: f32 = text
            .parse()
            .map_err(|_| format!("`{text}` is not a number in `{spec}`"))?;
        if !(v.is_finite() && (0.0..=1.0).contains(&v)) {
            return Err(format!("colour component `{text}` is outside 0.0-1.0"));
        }
        *slot = v;
    }
    Ok(pdfcer_core::vector::Rgb {
        r: c[0],
        g: c[1],
        b: c[2],
    })
}

/// Refuse a non-positive or non-finite metric BEFORE it reaches the model.
///
/// A zero text height or a negative stroke width is not a style, and the
/// sidecar reader would refuse to read it back anyway - so accepting it here
/// would write a document whose own next load silently ignores the value. A
/// refusal by name at the point of entry is the honest version of that.
pub(crate) fn checked_metric(name: &str, v: f64) -> Result<f64, String> {
    if v.is_finite() && v > 0.0 {
        Ok(v)
    } else {
        Err(format!("--{name} must be a positive number (got {v})"))
    }
}

/// `group-style` - set the group tier of the style cascade (Pass 69.0).
pub(crate) fn cmd_group_style(args: &GroupStyleArgs<'_>) -> u8 {
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let gid = pdfcer_core::dimension::GroupId(args.group);
    // Read-modify-write: flags not given must leave the existing value alone,
    // so the operator can set one property without knowing the other four.
    let Some(current) = session.dimension_model().group(gid).map(|g| g.style) else {
        eprintln!(
            "pdfcer: {}: no ce dimension group {} (see `dimension-list`)",
            args.input.display(),
            args.group
        );
        return exit::EDIT_REFUSED;
    };
    let mut style = if args.reset {
        pdfcer_core::dimension::GroupStyle::default()
    } else {
        current
    };
    for prop in args.clear {
        match prop {
            StylePropArg::TextHeight => style.text_height = None,
            StylePropArg::LineWidth => style.line_width = None,
            StylePropArg::ArrowLength => style.arrow_length = None,
            StylePropArg::ArrowForm => style.arrow_form = None,
            StylePropArg::Color => style.color = None,
            StylePropArg::Tolerance => style.tolerance = None,
            StylePropArg::TolerancePlaces => style.tolerance_places = None,
            // Named so the refusal says WHICH property and WHY, rather than
            // ignoring the flag and leaving the operator to discover from the
            // saved file that nothing happened.
            other => {
                eprintln!(
                    "pdfcer: {}: `{other:?}` is a per-ce-dimension property, not a group one -- \
                     the group's unit, precision, decimal marker and drafting standard are its own \
                     fields, set with `group-set-standard` and the scale commands",
                    args.input.display()
                );
                return exit::EDIT_REFUSED;
            }
        }
    }
    match apply_appearance(&args.appearance) {
        Ok(a) => {
            if let Some(v) = a.text_height {
                style.text_height = Some(v);
            }
            if let Some(v) = a.line_width {
                style.line_width = Some(v);
            }
            if let Some(v) = a.arrow_length {
                style.arrow_length = Some(v);
            }
            if let Some(v) = a.arrow_form {
                style.arrow_form = Some(v);
            }
            if let Some(v) = a.color {
                style.color = Some(v);
            }
            if let Some(v) = a.tolerance {
                style.tolerance = Some(v);
            }
            if let Some(v) = a.tolerance_places {
                style.tolerance_places = Some(v);
            }
        }
        Err(msg) => {
            eprintln!("pdfcer: {}: {msg}", args.input.display());
            return exit::EDIT_REFUSED;
        }
    }

    let members = match session.set_group_style(gid, style) {
        Ok(n) => n,
        Err(err) => return report_edit_error(args.input, &err),
    };
    let outcome = match save_edited(
        &mut session,
        &source,
        args.output,
        args.mode,
        ProducerArg::Preserve,
        args.verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "group-style {} group={} regenerated={members} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.group,
        args.mode.name(),
        args.output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(args.input, &outcome)
}

/// The five appearance values, validated and converted to core types.
pub(crate) struct ResolvedAppearance {
    pub(crate) text_height: Option<f64>,
    pub(crate) line_width: Option<f64>,
    pub(crate) arrow_length: Option<f64>,
    pub(crate) arrow_form: Option<pdfcer_core::dimension::ArrowForm>,
    pub(crate) color: Option<pdfcer_core::vector::Rgb>,
    pub(crate) tolerance: Option<pdfcer_core::dimension::Tolerance>,
    pub(crate) tolerance_places: Option<u32>,
}

/// Validate the appearance flags and convert them to core types.
///
/// # Errors
///
/// The operator-facing message naming the flag that failed.
pub(crate) fn apply_appearance(a: &AppearanceArgs) -> Result<ResolvedAppearance, String> {
    Ok(ResolvedAppearance {
        text_height: a
            .text_height
            .map(|v| checked_metric("text-height", v))
            .transpose()?,
        line_width: a
            .line_width
            .map(|v| checked_metric("line-width", v))
            .transpose()?,
        arrow_length: a
            .arrow_length
            .map(|v| checked_metric("arrow-length", v))
            .transpose()?,
        arrow_form: a.arrow_form.map(ArrowFormArg::to_core),
        color: a.color.as_deref().map(parse_style_color).transpose()?,
        tolerance: a.tolerance.as_deref().map(parse_tolerance).transpose()?,
        tolerance_places: match a.tolerance_places {
            Some(p) if p > 12 => {
                return Err(format!(
                    "--tolerance-places {p} is not a precision (max 12)"
                ));
            }
            other => other,
        },
    })
}

/// Parse a `--tolerance` spec (Pass 69.1).
///
/// Grammar, deliberately terse because it is typed by hand into a batch
/// script and read back in a diff:
///
/// ```text
/// none | basic | min | max
/// sym:<magnitude>            symmetric,  drawn ±v
/// dev:<plus>/<minus>         deviation,  drawn +p/-m   (both signed)
/// limit:<upper>/<lower>      limit,      drawn u/l     (nominal suppressed)
/// ```
///
/// Long forms (`symmetric:`, `deviation:`) are accepted for the same reason
/// the tokens are what the sidecar stores: a script that reads a value out of
/// `dimension-list --style` should be able to feed it straight back in.
///
/// Every parse runs through `Tolerance::validate`, so a refusal here says the
/// same thing the core API would say rather than a second, differently-worded
/// approximation of it.
pub(crate) fn parse_tolerance(spec: &str) -> Result<pdfcer_core::dimension::Tolerance, String> {
    use pdfcer_core::dimension::Tolerance;
    let s = spec.trim();
    let (head, rest) = s.split_once(':').map_or((s, ""), |(h, r)| (h, r));
    let num = |t: &str| -> Result<f64, String> {
        t.trim()
            .parse::<f64>()
            .map_err(|_| format!("`{t}` is not a number in --tolerance `{spec}`"))
    };
    let pair = |what: &str| -> Result<(f64, f64), String> {
        // `split_once('/')` and not `split('/')`: a negative number carries no
        // slash, so the FIRST slash is always the separator, and splitting on
        // all of them would accept `1/2/3` as if it meant something.
        let (a, b) = rest.split_once('/').ok_or_else(|| {
            format!("--tolerance {what} needs two values as `<{what}>:<a>/<b>`, got `{spec}`")
        })?;
        Ok((num(a)?, num(b)?))
    };
    let t = match head.to_ascii_lowercase().as_str() {
        "none" => Tolerance::None,
        "basic" => Tolerance::Basic,
        "min" => Tolerance::Min,
        "max" => Tolerance::Max,
        "sym" | "symmetric" => Tolerance::Symmetric {
            magnitude: num(rest)?,
        },
        "dev" | "deviation" => {
            let (plus, minus) = pair("dev")?;
            Tolerance::Deviation { plus, minus }
        }
        "limit" => {
            let (upper, lower) = pair("limit")?;
            Tolerance::Limit { upper, lower }
        }
        other => {
            return Err(format!(
                "unknown --tolerance `{other}` \
                 (none|basic|min|max|sym:<v>|dev:<p>/<m>|limit:<u>/<l>)"
            ));
        }
    };
    t.validate().map_err(|e| e.to_string())
}

/// `dimension-style` - set ONE ce dimension's overrides (Pass 69.0).
pub(crate) fn cmd_dimension_style(args: &DimensionStyleArgs<'_>) -> u8 {
    if args.places.is_some() && args.denominator.is_some() {
        eprintln!(
            "pdfcer: {}: --places and --denominator are two different number formats; give one",
            args.input.display()
        );
        return exit::EDIT_REFUSED;
    }
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let did = pdfcer_core::dimension::DimensionId(args.dimension);
    let Some(current) = session.dimension_model().dimension(did).map(|d| d.style) else {
        eprintln!(
            "pdfcer: {}: no ce dimension {} (see `dimension-list`)",
            args.input.display(),
            args.dimension
        );
        return exit::EDIT_REFUSED;
    };
    let mut style = if args.reset {
        pdfcer_core::dimension::StyleOverrides::default()
    } else {
        current
    };
    for prop in args.clear {
        match prop {
            StylePropArg::Unit => style.unit = None,
            StylePropArg::Fraction => style.fraction = None,
            StylePropArg::DecimalMarker => style.decimal_marker = None,
            StylePropArg::Standard => style.standard = None,
            StylePropArg::TextHeight => style.text_height = None,
            StylePropArg::LineWidth => style.line_width = None,
            StylePropArg::ArrowLength => style.arrow_length = None,
            StylePropArg::ArrowForm => style.arrow_form = None,
            StylePropArg::Color => style.color = None,
            StylePropArg::Tolerance => style.tolerance = None,
            StylePropArg::TolerancePlaces => style.tolerance_places = None,
        }
    }
    if let Some(token) = args.unit {
        let Some(unit) = pdfcer_core::dimension::Unit::parse(token) else {
            eprintln!(
                "pdfcer: {}: unknown --unit `{token}` (mm|cm|m|km|in|ft|ft-in|yd|mi)",
                args.input.display()
            );
            return exit::EDIT_REFUSED;
        };
        style.unit = Some(unit);
    }
    if let Some(places) = args.places {
        if places > 12 {
            eprintln!(
                "pdfcer: {}: --places {places} is not a precision (max 12)",
                args.input.display()
            );
            return exit::EDIT_REFUSED;
        }
        style.fraction = Some(pdfcer_core::dimension::FractionMode::Decimal { places });
    }
    if let Some(denominator) = args.denominator {
        if denominator == 0 || denominator > 4096 {
            eprintln!(
                "pdfcer: {}: --denominator {denominator} is out of range (1-4096)",
                args.input.display()
            );
            return exit::EDIT_REFUSED;
        }
        style.fraction = Some(pdfcer_core::dimension::FractionMode::Fraction {
            denominator,
            reduce: args.reduce,
        });
    }
    if let Some(m) = args.decimal_marker {
        style.decimal_marker = Some(m.to_core());
    }
    if let Some(std) = args.standard {
        style.standard = Some(std.to_core());
    }
    match apply_appearance(&args.appearance) {
        Ok(a) => {
            if let Some(v) = a.text_height {
                style.text_height = Some(v);
            }
            if let Some(v) = a.line_width {
                style.line_width = Some(v);
            }
            if let Some(v) = a.arrow_length {
                style.arrow_length = Some(v);
            }
            if let Some(v) = a.arrow_form {
                style.arrow_form = Some(v);
            }
            if let Some(v) = a.color {
                style.color = Some(v);
            }
            if let Some(v) = a.tolerance {
                style.tolerance = Some(v);
            }
            if let Some(v) = a.tolerance_places {
                style.tolerance_places = Some(v);
            }
        }
        Err(msg) => {
            eprintln!("pdfcer: {}: {msg}", args.input.display());
            return exit::EDIT_REFUSED;
        }
    }

    let overrides = match session.set_dimension_style(did, style) {
        Ok(n) => n,
        Err(err) => return report_edit_error(args.input, &err),
    };
    let outcome = match save_edited(
        &mut session,
        &source,
        args.output,
        args.mode,
        ProducerArg::Preserve,
        args.verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "dimension-style {} dimension={} overrides={overrides} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.dimension,
        args.mode.name(),
        args.output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(args.input, &outcome)
}

/// `dimension-list` — inventory the stored dimension model (read-only).
pub(crate) fn cmd_dimension_list(input: &Path, show_style: bool) -> u8 {
    use pdfcer_core::dimension::{DimensionKind, ScaleState};

    let doc = match open_for_read(input) {
        Ok(doc) => doc,
        Err(code) => return code,
    };
    let session = pdfcer_core::edit::EditSession::new(doc);
    let model = session.dimension_model();
    println!(
        "dimension-list {} groups={} dimensions={}",
        input.display(),
        model.groups().len(),
        model.dimensions().len()
    );
    for g in model.groups() {
        let scale = match g.scale {
            ScaleState::NeverSet => "no-scale".to_owned(),
            ScaleState::OneToOne => "1:1".to_owned(),
            ScaleState::Calibrated { scale } => format!("{scale} {}/pt", g.unit().abbrev()),
        };
        println!(
            "  group {} \"{}\" unit={} scale={scale} visible={} members={}",
            g.id.0,
            g.name,
            g.unit().token(),
            g.visible,
            model.member_count(g.id),
        );
        if show_style {
            // The group tier, printed as what it IS: a set of optional
            // defaults. `-` means the group has not spoken and the factory
            // default applies - deliberately not printed as the factory value,
            // because "10 (unset)" and "10 (set to 10)" behave differently the
            // moment the factory default changes.
            let st = g.style;
            println!(
                "    group-style text-height={} line-width={} arrow-length={} arrow-form={} color={}",
                opt_num(st.text_height),
                opt_num(st.line_width),
                opt_num(st.arrow_length),
                st.arrow_form
                    .map_or_else(|| "-".to_owned(), |f| f.token().to_owned()),
                opt_color(st.color),
            );
            // The group's DEFAULT tolerance (Pass 69.1), on its own line
            // because a tolerance spec carries slashes and colons and would be
            // unreadable wedged into the space-separated line above.
            println!(
                "    group-tolerance {} places={}",
                st.tolerance
                    .map_or_else(|| "-".to_owned(), format_tolerance_spec),
                opt_places(st.tolerance_places),
            );
        }
    }
    for d in model.dimensions() {
        let value = model.display(d.id).map_or_else(String::new, |m| m.text);
        let kind = match d.kind {
            DimensionKind::Linear { .. } => "linear",
            DimensionKind::Circular {
                show_diameter: true,
                ..
            } => "diameter",
            DimensionKind::Circular { .. } => "radius",
            DimensionKind::Angular { .. } => "angular",
            // Two tokens for one variant, deliberately: a script filtering for
            // fence runs and one filtering for pipe runs are looking for
            // different things, and `closed=` as a separate column would make
            // the common case a two-field test.
            DimensionKind::Perimeter { closed: true, .. } => "perimeter",
            DimensionKind::Perimeter { .. } => "path",
        };
        // The placement, for a linear dimension. Printed because it is
        // otherwise invisible from the CLI — an operator scripting
        // `subpath-delete`-style batch work cannot see WHERE a ce dimension
        // sits, only what it says, and `dimension-offset` below needs the
        // current values to adjust from.
        let placement = match d.kind {
            DimensionKind::Linear {
                offset, text_along, ..
            } => format!(" offset={offset} text_along={text_along}"),
            DimensionKind::Circular { .. } => String::new(),
            // An angular ce dimension's placement is a radius and a position
            // along the arc — the same one-drag pair as a linear one, in the
            // geometry an arc has. Reported under its own names rather than
            // reusing `offset=`, because a script reading `offset` from a
            // linear dimension and an angular one would be reading two
            // different quantities under one label.
            DimensionKind::Angular {
                radius, text_along, ..
            } => format!(" arc_radius={radius} text_along={text_along}"),
            // A perimeter reports its VERTEX COUNT alongside the placement
            // pair, because that count is the one fact a script driving
            // `dimension-vertex` needs and cannot get any other way: every
            // index it can pass is bounded by it.
            DimensionKind::Perimeter {
                ref points,
                offset,
                text_along,
                ..
            } => format!(
                " vertices={} offset={offset} text_along={text_along}",
                points.len()
            ),
        };
        // The override COUNT is printed unconditionally, and that is the
        // point: an operator scanning a list needs to see at a glance which ce
        // dimensions will move when he edits the group and which will not.
        // Printing nothing unless `--style` is passed would hide exactly the
        // surprise the cascade exists to prevent.
        // The TEXT OVERRIDE, printed unconditionally beside the measured
        // `value=` rather than replacing it (`Pass 175.0`, decision 097).
        //
        // Both, always, because that is what discloses the divergence:
        // `value=` stays the measurement and `label=` is what the page
        // actually prints. Replacing `value=` with the override would make
        // this listing agree with the page and disagree with the geometry,
        // and an operator auditing a drawing for overridden dimensions would
        // have nothing to compare. Omitting the override would do the reverse
        // and is worse — a caption that is not its measurement is exactly the
        // fact `CLAUDE.md` rule 4 says must not be silent.
        //
        // Absent when there is no override, so an un-overridden listing is
        // byte-identical to what this command printed before this Pass and no
        // existing script parsing it breaks.
        let label = model.label_override(d.id).map_or_else(String::new, |t| {
            format!(" label=\"{}\"", t.replace('"', "'"))
        });
        println!(
            "  dim {} group={} kind={kind} value=\"{value}\"{label}{placement} overrides={}",
            d.id.0,
            d.group.0,
            d.style.count()
        );
        if show_style {
            let Some(group) = model.group(d.group) else {
                continue;
            };
            let resolved = pdfcer_core::dimension::resolve_style(group, &d.style);
            let prov = pdfcer_core::dimension::style_provenance(group, &d.style);
            let values: [(&str, String); 11] = [
                ("unit", resolved.format.unit.token().to_owned()),
                ("fraction", format_fraction(resolved.format.fraction)),
                (
                    "decimal-marker",
                    resolved.format.decimal_marker.as_str().to_owned(),
                ),
                (
                    "standard",
                    format!("{:?}", resolved.standard).to_lowercase(),
                ),
                ("text-height", fmt_num(resolved.text_height)),
                ("line-width", fmt_num(resolved.line_width)),
                ("arrow-length", fmt_num(resolved.arrow_length)),
                ("arrow-form", resolved.arrow_form.token().to_owned()),
                (
                    "color",
                    format!(
                        "{},{},{}",
                        resolved.color.r, resolved.color.g, resolved.color.b
                    ),
                ),
                ("tolerance", format_tolerance_spec(resolved.tolerance)),
                (
                    "tolerance-places",
                    // `-` means "follow the nominal's precision", which is a
                    // different statement from any digit count and must not be
                    // printed as one.
                    opt_places(resolved.tolerance_places),
                ),
            ];
            // Paired positionally against `StyleProvenance::each`, whose own
            // doc comment explains why it returns a fixed-size array: a
            // property added without extending both sides is a compile error
            // rather than a silently shorter listing.
            let sources = prov.each();
            for ((name, value), (src_name, src)) in values.iter().zip(sources.iter()) {
                debug_assert_eq!(
                    name, src_name,
                    "value and provenance lists must stay aligned"
                );
                println!("    style {name}={value} ({})", src.token());
            }
        }
    }
    exit::SUCCESS
}

/// `Some(3.5)` as `3.5`, `None` as `-` (meaning: inherited, not set here).
pub(crate) fn opt_num(v: Option<f64>) -> String {
    v.map_or_else(|| "-".to_owned(), fmt_num)
}

/// A colour as `r,g,b`, or `-` when unset.
pub(crate) fn opt_color(c: Option<pdfcer_core::vector::Rgb>) -> String {
    c.map_or_else(|| "-".to_owned(), |c| format!("{},{},{}", c.r, c.g, c.b))
}

/// A metric with trailing zeros trimmed, so `10` reads as `10` rather than
/// `10.000000000000002`-adjacent noise in a listing an operator scans.
pub(crate) fn fmt_num(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() {
        "0".to_owned()
    } else {
        s.to_owned()
    }
}

/// A tolerance in the exact spec grammar `--tolerance` accepts back, so a
/// value read out of a listing can be fed straight into a script.
pub(crate) fn format_tolerance_spec(t: pdfcer_core::dimension::Tolerance) -> String {
    use pdfcer_core::dimension::Tolerance;
    match t {
        Tolerance::None | Tolerance::Basic | Tolerance::Min | Tolerance::Max => {
            t.token().to_owned()
        }
        Tolerance::Symmetric { magnitude } => format!("sym:{}", fmt_num(magnitude)),
        Tolerance::Deviation { plus, minus } => {
            format!("dev:{}/{}", fmt_num(plus), fmt_num(minus))
        }
        Tolerance::Limit { upper, lower } => {
            format!("limit:{}/{}", fmt_num(upper), fmt_num(lower))
        }
    }
}

/// A precision slot, or `-` for "follow the nominal's".
pub(crate) fn opt_places(p: Option<u32>) -> String {
    p.map_or_else(|| "-".to_owned(), |p| p.to_string())
}

/// The number format, in the vocabulary `dimension-style` accepts back.
pub(crate) fn format_fraction(f: pdfcer_core::dimension::FractionMode) -> String {
    match f {
        pdfcer_core::dimension::FractionMode::Decimal { places } => format!("{places}dp"),
        pdfcer_core::dimension::FractionMode::Fraction {
            denominator,
            reduce,
        } => format!("1/{denominator}{}", if reduce { " reduced" } else { "" }),
    }
}

/// `group-add` — create a named dimension group (Pass 12.M2).
pub(crate) fn cmd_group_add(
    input: &Path,
    name: &str,
    unit_str: &str,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    let Some(unit) = pdfcer_core::dimension::Unit::parse(unit_str) else {
        eprintln!(
            "pdfcer: {}: unknown --unit `{unit_str}` (mm|cm|m|in|ft|ft-in)",
            input.display()
        );
        return exit::EDIT_REFUSED;
    };
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let group = match session.add_dimension_group(name, unit) {
        Ok(id) => id,
        Err(err) => return report_edit_error(input, &err),
    };
    let outcome = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "group-add {} name=\"{name}\" unit={} mode={} -> {}; group={} changed={} objects={} \
appended={} out_bytes={}",
        input.display(),
        unit.token(),
        mode.name(),
        output.display(),
        group.0,
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
    );
    finish_edit(input, &outcome)
}

/// Borrowed argument bundle for [`cmd_group_set_scale`] (clippy arg-count).
pub(crate) struct GroupSetScaleArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) group: u32,
    pub(crate) real_length: Option<&'a str>,
    pub(crate) drawn: Option<f64>,
    pub(crate) ratio: Option<&'a str>,
    pub(crate) unit: &'a str,
    pub(crate) one_to_one: bool,
    pub(crate) precision: Option<u32>,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `group-set-scale` — set a group's scale + units and regenerate members.
pub(crate) fn cmd_group_set_scale(args: &GroupSetScaleArgs<'_>) -> u8 {
    use pdfcer_core::dimension::{
        GroupId, NumberFormat, ScaleEntry, ScaleState, Unit, parse_length, preview_group_scale,
    };

    let Some(unit) = Unit::parse(args.unit) else {
        eprintln!(
            "pdfcer: {}: unknown --unit `{}` (mm|cm|m|in|ft|ft-in)",
            args.input.display(),
            args.unit
        );
        return exit::EDIT_REFUSED;
    };
    // THE FORMAT IS BUILT AFTER THE SCALE BRANCH, NOT BEFORE IT
    // (`Pass 176.0`), because `--real-length` can name a unit that `--unit`
    // did not.
    //
    // It used to be built here, from `--unit` alone, and the `--real-length`
    // branch below then SHADOWED `unit` with the one it read out of the text.
    // The shadow was local to that block and the format had already been
    // computed outside it, so the text-named unit reached the SCALE and never
    // reached the LABEL.
    //
    // What that shipped, measured on the release binary 2026-08-30:
    //
    // ```text
    //   group-set-scale --real-length '55 5/8"' --drawn 200
    //   -> a 200 pt line reads   55.62 mm      (should be 55.62 in)
    // ```
    //
    // The magnitude is the INCH value and the label says MILLIMETRES — a
    // number that is neither, on a drawing, off by 25.4x. And this command's
    // own `--help` promises the opposite in writing: *"A notation that names a
    // unit sets the group's unit too … the same rule the GUI field follows, so
    // a command and a click produce the same result from the same text."*
    //
    // So the branch now yields `(scale, effective_unit)` and the format is
    // built from the unit that actually won. One binding, resolved once,
    // consumed once -- there is no longer a second `unit` for a reader to pick
    // the wrong one of.
    let (scale, unit) = if args.one_to_one {
        (ScaleState::OneToOne, unit)
    } else if let Some(ratio) = args.ratio {
        let Some((paper, real)) = parse_ratio(ratio) else {
            eprintln!("pdfcer: {}: --ratio must be `N:M`", args.input.display());
            return exit::EDIT_REFUSED;
        };
        // A ratio carries no unit of its own -- `1:100` is dimensionless --
        // so `--unit` is the only source here and passes through unchanged.
        match preview_group_scale(ScaleEntry::Ratio {
            paper,
            real,
            basis: unit,
        }) {
            Some(p) => (ScaleState::Calibrated { scale: p.scale }, unit),
            None => {
                eprintln!("pdfcer: {}: invalid ratio", args.input.display());
                return exit::EDIT_REFUSED;
            }
        }
    } else if let (Some(real_text), Some(drawn)) = (args.real_length, args.drawn) {
        // Parsed with the SAME function the GUI field uses, so `55 5/8"` means
        // one thing in this product rather than two. A second, CLI-local
        // number parser would be a duplicated predicate and would drift (R92)
        // — and it would drift silently, because both would keep accepting
        // plain decimals long after they disagreed about fractions.
        let parsed = match parse_length(real_text, unit) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("pdfcer: --real-length: {e}");
                return exit::EDIT_REFUSED;
            }
        };
        // A unit named in the text wins over `--unit`, matching the GUI: the
        // operator said inches by typing `"`, and making them repeat it in a
        // flag would be asking the same question twice.
        //
        // This binding is no longer a SHADOW -- it is the value the whole
        // expression yields, so it reaches the number format as well as the
        // scale. See the note above the branch for what the shadow cost.
        let unit = if parsed.unit_from_text {
            parsed.unit
        } else {
            unit
        };
        match preview_group_scale(ScaleEntry::RealLength {
            drawn_pdf_length: drawn,
            real_length: parsed.value,
            unit,
        }) {
            Some(p) => (ScaleState::Calibrated { scale: p.scale }, unit),
            None => {
                eprintln!(
                    "pdfcer: {}: --drawn must be a positive length",
                    args.input.display()
                );
                return exit::EDIT_REFUSED;
            }
        }
    } else {
        eprintln!(
            "pdfcer: {}: give --one-to-one, --ratio N:M, or --real-length L --drawn D",
            args.input.display()
        );
        return exit::EDIT_REFUSED;
    };

    // Built from the unit that WON, which for `--real-length '4\'-7 1/2"'` is
    // `ft-in` and takes the feet-inches arm below -- an operator writing
    // architectural notation gets architectural output without also passing
    // `--unit ft-in`.
    let format = match args.precision {
        Some(p) if unit == Unit::FeetInches => NumberFormat::feet_inches(p, false),
        Some(p) => NumberFormat::decimal(unit, p),
        None => unit.default_format(),
    };

    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let members = match session.set_group_scale(GroupId(args.group), scale, format) {
        Ok(n) => n,
        Err(err) => return report_edit_error(args.input, &err),
    };
    let outcome = match save_edited(
        &mut session,
        &source,
        args.output,
        args.mode,
        ProducerArg::Preserve,
        args.verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "group-set-scale {} group={} unit={} mode={} -> {}; members_regenerated={members} \
changed={} objects={} appended={} out_bytes={}",
        args.input.display(),
        args.group,
        // REPORTED (`Pass 176.0`), and its absence is part of why the unit
        // bug survived: this line said what was set for every field except the
        // one that was wrong. A caller passing `--real-length '55 5/8"'` saw a
        // success line with no unit on it and had no reason to check.
        unit.token(),
        args.mode.name(),
        args.output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
    );
    finish_edit(args.input, &outcome)
}
