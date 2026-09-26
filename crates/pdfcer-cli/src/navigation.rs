use super::*;

/// Implement `pdfcer list-annotations <input> [--pages …]`: a read-only
/// per-page annotation inventory (ISO 32000-1 §12.5).
///
/// # Output (locale-invariant, stable, parseable)
///
/// One `annot …` line per annotation, in page then `/Annots`-array order
/// (deterministic), followed by one `list-annotations …` summary line.
/// Both are pure-ASCII `key=value` and go to **stdout**; the leading token
/// distinguishes them. Per line:
///
/// ```text
/// annot page=<P> index=<I> subtype=<Name|none> rect=<llx,lly,urx,ury|none> \
///       flags=0x<hex> widget=<0|1> disposition=<D> ap=<A> \
///       author=<none|"…"> note=<none|"…"> modified=<none|"…">
/// list-annotations <input> pages=<N>; annots=<T> paint_ready=<P> no_ap=<Q> \
///       state_missing=<S> suppressed=<H> popup=<U> widget=<W> \
///       with_note=<C> with_author=<A> need_appearances=<0|1>
/// ```
///
/// `disposition` is the **model-level** classification a reader would
/// apply, in the render path's precedence order:
/// - `popup` — a `/Popup`: never page content (§12.5.6.14), whatever its
///   `/AP`.
/// - `suppressed` — Hidden or NoView flag set (§12.5.3): not shown on
///   screen (R50: disclosed anyway).
/// - `paint-ready` — a resolvable normal appearance stream (`render-page`
///   would paint it, unless its transformed box is degenerate — a
///   placement fact `render-page`'s `annots_degenerate` counter reports).
/// - `no-ap` — no usable `/AP` `/N` (R43 named-not-painted).
/// - `state-missing` — an `/AS` that could not be resolved (§12.5.5 NOTE 3).
/// - `no-rect` — a paintable appearance but no `/Rect` placement target.
///
/// `ap` is the appearance shape: `stream`, `state-dict`, or `none`.
///
/// # The three note columns (Pass 38.5, closing this command's own named gap)
///
/// `author`, `note` and `modified` are `/T`, `/Contents` and `/M`
/// (§12.5.2 Table 164, §12.5.6.2 Table 170), decoded by
/// [`pdfcer_core::annot`]'s §7.9.2 text-string reader so a UTF-16BE
/// `/Contents` prints as text and not as mojibake. They are appended
/// **last**, after `ap=`, so a parser that reads through the pre-existing
/// columns is unaffected.
///
/// Each is either the bare token `none` or a `quoted_token` string, and
/// the distinction is load-bearing rather than cosmetic — a document
/// really can carry the literal author name `none`, and it prints as
/// `author="none"`. Quoting also makes a note containing spaces,
/// newlines or quotes a single field, which an unquoted value could not
/// be.
///
/// **`author=none` never means "anonymous".** `/T` is a **Table 170
/// markup-only** key: a `/Link` or a `/Widget` has no author concept at
/// all, so its absence there is a statement about the subtype, not about
/// the person. The same distinction the core model draws
/// ([`pdfcer_core::annot::Annotation::title`]) is preserved here rather
/// than flattened into an empty string.
///
/// **`modified` is emitted RAW**, exactly as the file stores it, because
/// §12.5.2 types `/M` as *"date **or** text string"* and obliges a reader
/// to accept any format. Normalising it to ISO-8601 here would invent
/// precision the document does not have, and would silently discard the
/// producer-specific formats that are the whole reason the key is typed
/// so loosely.
///
/// The summary line's `with_note` / `with_author` counts are over the
/// **selected pages only**, like every other counter on that line.
///
/// # Exit codes
///
/// `0` success; `3`/`4` unreadable / not-a-PDF; `1` for a structural
/// failure or an out-of-range `--pages` selection.
pub(crate) fn cmd_list_annotations(input: &Path, pages_spec: &str) -> u8 {
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
    let selected = match parse_pages(pages_spec, pages.len()) {
        Ok(sel) => sel,
        Err(msg) => {
            eprintln!("pdfcer: {}: {msg}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };

    let need_appearances = usize::from(pdfcer_core::annot::need_appearances(&doc));
    let (mut total, mut paint_ready, mut no_ap) = (0usize, 0usize, 0usize);
    let (mut state_missing, mut suppressed, mut popup, mut widget) =
        (0usize, 0usize, 0usize, 0usize);
    let (mut with_note, mut with_author) = (0usize, 0usize);

    for &page_index in &selected {
        let Some(page) = pages.get(page_index) else {
            continue;
        };
        let annots = pdfcer_core::annot::page_annotations(&doc, page.id);
        for (array_index, annot) in annots.iter().enumerate() {
            total += 1;
            if annot.is_widget() {
                widget += 1;
            }
            let (disposition, ap_shape) = classify_for_listing(annot);
            match disposition {
                "popup" => popup += 1,
                "suppressed" => suppressed += 1,
                "paint-ready" | "no-rect" => paint_ready += 1,
                "no-ap" => no_ap += 1,
                "state-missing" => state_missing += 1,
                _ => {}
            }
            let subtype = if annot.subtype.is_empty() {
                "none".to_owned()
            } else {
                // Names cannot legally contain whitespace; sanitise
                // defensively so the line stays field-splittable.
                sanitize_token(&annot.subtype_label())
            };
            let rect = match annot.rect {
                Some(r) => format!("{},{},{},{}", r.llx, r.lly, r.urx, r.ury),
                None => "none".to_owned(),
            };
            if annot.contents.is_some() {
                with_note += 1;
            }
            if annot.title.is_some() {
                with_author += 1;
            }
            // `none` (bare) vs `"…"` (quoted): see this function's doc
            // comment — the bare token is what makes an ABSENT key
            // distinguishable from a key whose value happens to be the
            // word "none".
            let opt_token = |v: Option<&String>| match v {
                Some(s) => quoted_token(s),
                None => "none".to_owned(),
            };
            // `Pass 255.0`: the point geometry a shell draws reshape anchors
            // from, in the same `x,y;x,y` spelling `annotate` accepts, so
            // the output of this command can be fed back to
            // `annotation-vertex --at`. `none` when the key is absent.
            let pts = |v: &[(f64, f64)]| {
                v.iter()
                    .map(|(x, y)| format!("{x},{y}"))
                    .collect::<Vec<_>>()
                    .join(";")
            };
            let vertices = annot
                .vertices
                .as_deref()
                .map_or_else(|| "none".to_owned(), pts);
            let line = annot
                .line
                .as_ref()
                .map_or_else(|| "none".to_owned(), |l| pts(&l[..]));
            let ink = annot.ink_list.as_deref().map_or_else(
                || "none".to_owned(),
                |strokes| strokes.iter().map(|s| pts(s)).collect::<Vec<_>>().join("|"),
            );
            // `Pass 292.0`: what a placed STAMP was drawn with. Appended to
            // the line, never inserted, per the stable-line contract.
            //
            // `none` for every other subtype and for a stamp whose appearance
            // pdfcer cannot describe -- Acrobat's custom stamps are artwork
            // rather than a laid-out label, and `none` is the honest answer to
            // "what size is this stamp's text", not a failure. The SOURCE is
            // printed beside the number because "the author stated 12pt" and
            // "pdfcer read 12pt off the picture" are different facts, and
            // "the author stated something unreadable" is a third.
            let stamp_params = annot
                .id
                .and_then(|id| pdfcer_core::graph::ObjectGraph::value(&doc, id))
                .and_then(pdfcer_core::object::Object::as_dict)
                .and_then(|d| {
                    pdfcer_core::annot::stamp_label_parameters_in(
                        &doc,
                        pdfcer_core::view::StreamSource::Contiguous(doc.bytes()),
                        d,
                    )
                });
            let (stamp_label, stamp_size, stamp_size_from) = match &stamp_params {
                Some(p) => (
                    quoted_token(&p.label),
                    format!("{:.2}", p.size),
                    match p.size_source {
                        pdfcer_core::annot::StampSizeSource::DeclaredInDa => "da",
                        pdfcer_core::annot::StampSizeSource::RecoveredFromAppearance => {
                            "appearance"
                        }
                        pdfcer_core::annot::StampSizeSource::DaUnreadable => "da-unreadable",
                        // `#[non_exhaustive]`: a new source must print
                        // SOMETHING rather than fail to compile a shell.
                        _ => "other",
                    }
                    .to_owned(),
                ),
                None => ("none".to_owned(), "none".to_owned(), "none".to_owned()),
            };
            println!(
                "annot page={} index={array_index} subtype={subtype} rect={rect} \
flags=0x{:X} widget={} disposition={disposition} ap={ap_shape} action={} author={} note={} modified={} open={} color={} icon={} vertices={vertices} line={line} ink={ink} stamp_label={stamp_label} stamp_size={stamp_size} stamp_size_from={stamp_size_from}",
                page_index + 1,
                annot.flags.0,
                usize::from(annot.is_widget()),
                // `Pass 133.0`. WHAT THIS ANNOTATION DOES WHEN CLICKED —
                // the one entry in Table 164 that describes a consequence
                // for the operator, and the one this line used to omit. A
                // widget that submits a form to a web server printed
                // identically to one that does nothing.
                annot.action_type.as_deref().map_or_else(
                    || "none".to_owned(),
                    |a| {
                        let name = sanitize_token(&String::from_utf8_lossy(a));
                        // `+next` is NOT decoration. A `/GoTo` that chains to
                        // a `/SubmitForm` reads as an ordinary navigation
                        // link without it, which is a disclosure that
                        // MISLEADS rather than one that is merely thin.
                        if annot.action_chains {
                            format!("{name}+next")
                        } else {
                            name
                        }
                    },
                ),
                opt_token(annot.title.as_ref()),
                opt_token(annot.contents.as_ref()),
                opt_token(annot.mod_date.as_ref()),
                // `/Open` (Pass 259.0). THREE values, not two: `none` means
                // the file carried no such key, which is a different fact
                // from `0` and the reason the model uses `Option<bool>` --
                // a geometric markup has no `/Open` of its own and keeps
                // its window state on its `/Popup` companion, listed here
                // as its own row.
                match annot.open {
                    Some(true) => "1",
                    Some(false) => "0",
                    None => "none",
                },
                // `/C` as the RAW components, space-free so the field stays
                // one token. The COUNT is the colour space (Table 164), so
                // `0.2,0.4` -- two components, which no space defines -- is
                // printed as it is rather than repaired: an operator who
                // sees it is seeing the file's actual malformation.
                // `empty` is the standard's own "no colour"; `none` is an
                // absent key. Three states, as the model has.
                match annot.color.as_deref() {
                    None => "none".to_owned(),
                    Some([]) => "empty".to_owned(),
                    Some(c) => c
                        .iter()
                        .map(|v| format!("{v}"))
                        .collect::<Vec<_>>()
                        .join(","),
                },
                // `/Name`, raw -- §12.5.6.4's set is open, so a producer's
                // own icon name prints as itself.
                annot.icon.as_deref().map_or_else(
                    || "none".to_owned(),
                    |n| sanitize_token(&String::from_utf8_lossy(n)),
                ),
            );
        }
    }

    println!(
        "list-annotations {} pages={}; annots={total} paint_ready={paint_ready} no_ap={no_ap} \
state_missing={state_missing} suppressed={suppressed} popup={popup} widget={widget} \
with_note={with_note} with_author={with_author} need_appearances={need_appearances}",
        input.display(),
        selected.len(),
    );
    exit::SUCCESS
}

/// `tab-order`: print the order a reader visits each selected page's
/// annotations in (ISO 32000-1 §12.5.1).
///
/// ## Stable line format
///
/// ```text
/// page-order page=<P> tabs=<R|C|S|A|W|absent|/Name> basis=<...> derived=<0|1> \
///            rotate=<0|90|180|270> direction=<l2r|r2l> visited=<n> skipped=<n> pinned=<n>
/// tab page=<P> visit=<1-based> obj=<num> subtype=<Name|none> rect=<llx,lly,urx,ury|none> field=<"…"|none>
/// skipped page=<P> obj=<num> subtype=<Name|none> why=<hidden|no-view|trap-net|popup>
/// note page=<P> text=<"…">
/// tab-order <path> pages=<n>; visited=<n> skipped=<n> pinned=<n> derived_pages=<n> not_derived_pages=<n>
/// ```
///
/// Every field is on one line and space-free, like every other inventory
/// command here, so `cut`/`awk` keep working.
///
/// ## Why `note` lines are on stdout and not stderr
///
/// They are not diagnostics. Under `basis=row`, `basis=column` and
/// `basis=array-convention` the sequence above them is **pdfcer's
/// inference**, and rule 4 makes disclosing that part of the answer rather
/// than commentary on it. A script that pipes stdout to a file and discards
/// stderr would otherwise keep the order and lose the fact that pdfcer
/// worked it out — which is precisely the silence the rule forbids. `pdfcer`
/// has no session and no undo, so printing on the way past is the whole of
/// its disclosure (project rule 11).
///
/// ## Exit codes
///
/// `0` success; `3`/`4` unreadable / not-a-PDF; `1` for a structural failure
/// or an out-of-range `--pages` selection.
pub(crate) fn cmd_tab_order(input: &Path, pages_spec: &str, row_tolerance: Option<f64>) -> u8 {
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
    let selected = match parse_pages(pages_spec, pages.len()) {
        Ok(sel) => sel,
        Err(msg) => {
            eprintln!("pdfcer: {}: {msg}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };

    let mut session = pdfcer_core::edit::EditSession::new(doc);
    // The two ambiguity settings this command depends on, read in the SHELL
    // and handed to the session -- core never opens a settings file (R83,
    // and the same convention `--unmappable-code` and `quad_point_order`
    // follow). `--row-tolerance` overrides the store for this run only.
    let (settings, settings_report) =
        pdfcer_core::settings::Settings::load(pdfcer_core::settings::resolve_store());
    report_settings(&settings_report);
    session.set_widget_tab_tail(settings.widget_tab_tail);
    session.set_tab_row_tolerance(row_tolerance.unwrap_or(settings.tab_row_tolerance));

    let (mut visited, mut skipped, mut pinned) = (0usize, 0usize, 0usize);
    let (mut derived_pages, mut not_derived_pages) = (0usize, 0usize);

    for &page_index in &selected {
        let seq = match session.page_tab_sequence(page_index) {
            Ok(seq) => seq,
            Err(err) => {
                eprintln!("pdfcer: {}: {err}", input.display());
                return exit::RUNTIME_ERROR;
            }
        };
        let page = page_index + 1;
        let tabs = match &seq.stated {
            pdfcer_core::edit::PageTabs::Absent => "absent".to_owned(),
            pdfcer_core::edit::PageTabs::Row => "R".to_owned(),
            pdfcer_core::edit::PageTabs::Column => "C".to_owned(),
            pdfcer_core::edit::PageTabs::Structure => "S".to_owned(),
            pdfcer_core::edit::PageTabs::ArrayOrder => "A".to_owned(),
            pdfcer_core::edit::PageTabs::WidgetOrder => "W".to_owned(),
            // Verbatim, and sanitised only so the line stays splittable --
            // a disclosure that names the value the file carries can be
            // checked against the file; "unknown" cannot.
            pdfcer_core::edit::PageTabs::Other(name) => format!("/{}", sanitize_token(name)),
            // `PageTabs` is #[non_exhaustive]. A value a future core knows
            // and this build does not prints as `unrecognised` rather than
            // as one of the names above — a wrong name here would be read
            // as a fact about the file.
            _ => "unrecognised".to_owned(),
        };
        let basis = match seq.basis {
            pdfcer_core::edit::TabOrderBasis::StatedArrayOrder => "stated-array",
            pdfcer_core::edit::TabOrderBasis::StatedWidgetOrder => "stated-widget",
            pdfcer_core::edit::TabOrderBasis::ComputedRowOrder => "row",
            pdfcer_core::edit::TabOrderBasis::ComputedColumnOrder => "column",
            pdfcer_core::edit::TabOrderBasis::ArrayOrderByConvention => "array-convention",
            pdfcer_core::edit::TabOrderBasis::NotDerivedStructure => "not-derived",
            _ => "unrecognised",
        };
        if matches!(
            seq.basis,
            pdfcer_core::edit::TabOrderBasis::NotDerivedStructure
        ) {
            not_derived_pages += 1;
        } else if seq.derived {
            derived_pages += 1;
        }
        println!(
            "page-order page={page} tabs={tabs} basis={basis} derived={} rotate={} \
direction={} visited={} skipped={} pinned={}",
            u8::from(seq.derived),
            seq.rotate,
            if seq.right_to_left { "r2l" } else { "l2r" },
            seq.order.len(),
            seq.excluded.len(),
            seq.pinned,
        );

        let describe = |id: pdfcer_core::object::ObjId| {
            let dict = session
                .value(id)
                .and_then(pdfcer_core::object::Object::as_dict);
            let subtype = dict
                .and_then(|d| d.get(b"Subtype"))
                .and_then(pdfcer_core::object::Object::as_name)
                .map_or_else(
                    || "none".to_owned(),
                    |n| sanitize_token(&String::from_utf8_lossy(n.as_bytes())),
                );
            let rect = dict
                .and_then(|d| d.get(b"Rect"))
                .and_then(pdfcer_core::object::Object::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(pdfcer_core::object::Object::as_number)
                        .map(|v| format!("{v}"))
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "none".to_owned());
            (subtype, rect)
        };

        for (n, &id) in seq.order.iter().enumerate() {
            visited += 1;
            let (subtype, rect) = describe(id);
            println!(
                "tab page={page} visit={} obj={} subtype={subtype} rect={rect}",
                n + 1,
                id.num,
            );
        }
        for &(id, why) in &seq.excluded {
            skipped += 1;
            let (subtype, _) = describe(id);
            let reason = match why {
                pdfcer_core::edit::TabExclusion::Hidden => "hidden",
                pdfcer_core::edit::TabExclusion::NoView => "no-view",
                pdfcer_core::edit::TabExclusion::TrapNet => "trap-net",
                pdfcer_core::edit::TabExclusion::Popup => "popup",
                _ => "unrecognised",
            };
            println!(
                "skipped page={page} obj={} subtype={subtype} why={reason}",
                id.num
            );
        }
        pinned += seq.pinned;
        for note in &seq.notes {
            println!("note page={page} text={}", quoted_token(note));
        }
    }

    println!(
        "tab-order {} pages={}; visited={visited} skipped={skipped} pinned={pinned} \
derived_pages={derived_pages} not_derived_pages={not_derived_pages}",
        input.display(),
        selected.len(),
    );
    exit::SUCCESS
}

/// `list-links`: resolve every `/Link` annotation's destination
/// (ISO 32000-1 §12.5.6.5, Table 173) across the selected pages.
///
/// ## Why this is not a column on `list-annotations`
///
/// `list-annotations` prints the action's `/S` name and stops, and that
/// is the right disclosure for an inventory: it is free to read, it
/// answers *"what does this do to me"*, and it never walks the document.
/// Resolving where the action **points** costs a page-tree walk plus a
/// flatten of both §12.3.2.3 named-destination namespaces. Bolting that
/// onto `list-annotations` would make every inventory of every document
/// pay for a question almost no inventory asks.
///
/// So the cost lives here, where it is asked for, and is paid **once**:
/// [`DestinationReader`] is built before the page loop, not inside it.
/// Building one per page would walk the page tree once per page — the
/// quadratic shape this crate has been bitten by before.
///
/// ## Output contract
///
/// One `link` line per resolved link, then exactly one `links` summary
/// line. Field order is fixed and every value is a single whitespace-free
/// token or a quoted string, so `awk '$2 ~ /page=/'` keeps working.
///
/// The summary line is printed **unconditionally**, including when both
/// its counts are zero. That is deliberate: a tool that prints nothing
/// for a document with no links is indistinguishable from a tool that
/// failed to look, and this crate has shipped that exact ambiguity
/// before.
///
/// ## Exit codes
///
/// `0` always, when the document opened — a document with no links is
/// not an error, it is an answer. Open failures map through
/// [`exit_code_for_doc`]; an unwalkable page tree is
/// [`exit::RUNTIME_ERROR`], because "which page is this" has no answer
/// at all then and every line would be a lie of omission.
pub(crate) fn cmd_list_links(input: &Path, pages_spec: &str) -> u8 {
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
    let selected = match parse_pages(pages_spec, pages.len()) {
        Ok(sel) => sel,
        Err(msg) => {
            eprintln!("pdfcer: {}: {msg}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };

    // Built ONCE. See this function's doc comment.
    let reader = DestinationReader::new(&doc);
    if let Some(err) = reader.page_tree_error() {
        // Reachable only if `page_slots` fails where `pages` succeeded,
        // which their differing strictness makes possible in principle.
        // Disclosed rather than swallowed: with no page map, EVERY
        // explicit destination below reports `unmapped` whether or not
        // its target exists, and blaming the document for that would be
        // the reader accusing the file of its own blindness.
        eprintln!(
            "pdfcer: {}: page tree unreadable ({err}); every destination \
will report unmapped",
            input.display()
        );
    }

    let (mut resolved, mut broken) = (0usize, 0usize);
    for &page_index in &selected {
        let Some(page) = pages.get(page_index) else {
            continue;
        };
        let found = pdfcer_core::annot::page_link_destinations(&doc, page.id, &reader);
        broken += found.links_without_destination;
        for link in &found.links {
            resolved += 1;
            let rect = match link.rect {
                Some(r) => format!("{},{},{},{}", r.llx, r.lly, r.urx, r.ury),
                None => "none".to_owned(),
            };
            let detail = describe_destination(&link.destination);
            println!(
                "link page={} index={} rect={rect} {detail}",
                page_index + 1,
                link.annots_index,
            );
        }
    }
    println!("links resolved={resolved} links-without-destination={broken}");
    exit::SUCCESS
}

/// Render one [`Destination`] as the `dest=…` field group of a
/// `list-links` line.
///
/// Every variant gets a **distinct** `dest=` token, and none of them is
/// `dest=none`. That is the point of the function: a viewer or a script
/// that collapsed `remote`, `named`, `unmapped` and `action` into "no
/// destination" would report a document full of working links as a
/// document full of nothing, and a script that collapsed them into
/// `page` would send the operator somewhere arbitrary. The five tokens
/// are the disclosure.
pub(crate) fn describe_destination(destination: &Destination) -> String {
    match destination {
        Destination::Page { page_index, view } => {
            // 1-based on the way out, matching every other page= this
            // CLI prints; 0-based is a core-internal convention.
            format!(
                "dest=page target={} view={}",
                page_index + 1,
                view_token(view)
            )
        }
        Destination::UnmappedPage { page, view } => {
            let target = match page {
                Some(id) => format!("{} {}", id.num, id.generation),
                None => "none".to_owned(),
            };
            format!(
                "dest=unmapped target={} view={}",
                quoted_token(&target),
                view_token(view)
            )
        }
        Destination::Named { name } => format!(
            "dest=named name={}",
            quoted_token(&String::from_utf8_lossy(name))
        ),
        Destination::Remote {
            file,
            target,
            view,
            new_window,
        } => {
            let file = match file {
                Some(bytes) => quoted_token(&String::from_utf8_lossy(bytes.as_slice())),
                None => "none".to_owned(),
            };
            let target = match target {
                RemoteTarget::PageNumber(number) => format!("page:{number}"),
                RemoteTarget::Named(bytes) => {
                    format!("name:{}", sanitize_token(&String::from_utf8_lossy(bytes)))
                }
                _ => "unknown".to_owned(),
            };
            // §12.6.4.3 leaves `/NewWindow` absent-means-viewer's-choice,
            // so `unset` is a third value, not a synonym for `0`.
            let window = match new_window {
                Some(true) => "new",
                Some(false) => "same",
                None => "unset",
            };
            format!(
                "dest=remote file={file} target={target} window={window} view={}",
                view_token(view)
            )
        }
        Destination::NonNavigation { action, file } => {
            let action = match action {
                Some(name) => sanitize_token(&String::from_utf8_lossy(name.as_bytes())),
                None => "unreadable".to_owned(),
            };
            // A `/Launch` names a file, and printing it is the whole point
            // of reading it (§12.6.4.5). Omitted rather than printed as
            // `none` for the actions that name no file, so the key's
            // presence means something.
            match file {
                Some(bytes) => format!(
                    "dest=action action={action} file={}",
                    quoted_token(&String::from_utf8_lossy(bytes.as_slice()))
                ),
                None => format!("dest=action action={action}"),
            }
        }
        // `Destination` is `#[non_exhaustive]`; a variant added later
        // must not silently become one of the five above.
        _ => "dest=unrecognised".to_owned(),
    }
}

/// The Table 151 fit style as one lowercase-free token.
///
/// The style only — not its parameters. A `/XYZ`'s left/top/zoom and a
/// `/FitR`'s rectangle are four to five more numbers per line, and a
/// caller that needs them is a viewer, which should be calling
/// `pdfcer-core` rather than parsing this. What a *script* needs from a
/// CLI line is whether the link frames a region or fits the sheet.
pub(crate) fn view_token(view: &DestView) -> &'static str {
    match view {
        DestView::Absent => "absent",
        DestView::Xyz { .. } => "XYZ",
        DestView::Fit => "Fit",
        DestView::FitH { .. } => "FitH",
        DestView::FitV { .. } => "FitV",
        DestView::FitR { .. } => "FitR",
        DestView::FitB => "FitB",
        DestView::FitBH { .. } => "FitBH",
        DestView::FitBV { .. } => "FitBV",
        DestView::Unknown { .. } => "unknown",
        _ => "unrecognised",
    }
}

/// `add-bookmark` — append one item to the document outline (§12.3.3).
///
/// # Resolving `--under`, and why it is an index rather than a title
///
/// The obvious interface is `--under "Chapter 1"`, and it is the wrong one:
/// outline titles are **not unique** — "Introduction" under each of three
/// parts is ordinary — so a title selects an ambiguous set, and the command
/// would have to either refuse the common case or silently pick one. The
/// `n=` index `list-outline` prints is unambiguous by construction, and the
/// two commands compose without the operator inventing a path syntax.
///
/// The index is resolved through the **reader**, not by counting objects, so
/// `--under` names exactly the row the operator saw. A tree pdfcer truncated
/// at `MAX_OUTLINE_DEPTH` or cut short on its item budget therefore has no
/// reachable indices past that point — which is right: an index into a row
/// the operator was never shown would nest the bookmark somewhere invisible.
///
/// # What it prints
///
/// `root_count=` is the outline root's `/Count` after the add — the number
/// of *visible* items at every level. It does **not** always increase: an
/// item added under a collapsed ancestor is not visible, so the count is
/// unchanged and the command says `visible=0`. Reporting the count without
/// that flag would make a correct save look like a failed one.
#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_add_bookmark(
    input: &Path,
    title: &str,
    page: Option<u32>,
    top: Option<f64>,
    under: Option<usize>,
    dest_name: Option<&str>,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    if top.is_some() && page.is_none() {
        eprintln!(
            "pdfcer: {}: --top positions a view WITHIN a page; it needs --page",
            input.display()
        );
        return exit::EDIT_REFUSED;
    }

    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    // The destination. `--page` alone means /Fit — fit the whole page —
    // rather than /XYZ with null parameters. Both are legal and the
    // difference is real: /XYZ null null null means "keep whatever the
    // viewer is showing", which for a bookmark authored from a script with
    // no view to capture would make the jump depend on where the reader
    // happened to be. /Fit always shows the page the operator named.
    let destination = match page {
        // clap enforces the mutual exclusion, so reaching here with both is
        // impossible; a named destination therefore only competes with the
        // no-destination case.
        None if dest_name.is_some() => {
            dest_name.map(|n| pdfcer_core::outline::Destination::Named {
                name: n.as_bytes().to_vec(),
            })
        }
        None => None,
        Some(p) => {
            let Some(index) = p.checked_sub(1).map(|i| i as usize) else {
                eprintln!(
                    "pdfcer: {}: --page is 1-based; 0 is not a page",
                    input.display()
                );
                return exit::EDIT_REFUSED;
            };
            let view = match top {
                Some(y) => pdfcer_core::outline::DestView::Xyz {
                    left: None,
                    top: Some(y),
                    zoom: None,
                },
                None => pdfcer_core::outline::DestView::Fit,
            };
            Some(pdfcer_core::outline::Destination::Page {
                page_index: index,
                view,
            })
        }
    };

    // The parent, resolved through the reader in the same depth-first order
    // `list-outline` prints.
    let before = pdfcer_core::outline::read_outline(&session.graph());
    let parent = match under {
        None => None,
        Some(n) => {
            let Some(id) = nth_outline_item(&before.items, n) else {
                eprintln!(
                    "pdfcer: {}: --under {n}: this document has {} bookmark(s); \
run list-outline to see their n= numbers",
                    input.display(),
                    count_outline_items(&before.items),
                );
                return exit::EDIT_REFUSED;
            };
            Some(id)
        }
    };

    let before_root = before.diagnostics.declared_root_count.unwrap_or(0);
    if let Err(err) = session.add_outline_item(parent, title, destination) {
        return report_edit_error(input, &err);
    }
    let after = pdfcer_core::outline::read_outline(&session.graph());
    let after_root = after.diagnostics.declared_root_count.unwrap_or(0);

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
        "add-bookmark {} title={title:?} mode={} -> {}; \
bookmarks={} root_count={after_root} visible={} changed={} objects={} \
verbatim={} reserialized={} appended={} out_bytes={} undo_verified={} \
undo_identical={} delinearized={}",
        input.display(),
        mode.name(),
        output.display(),
        count_outline_items(&after.items),
        u32::from(after_root != before_root),
        outcome.changed,
        r.objects_written,
        r.objects_verbatim,
        r.objects_reserialized,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
        u32::from(r.delinearized),
    );
    finish_edit(input, &outcome)
}

/// `rename-bookmark` and `delete-bookmark` — one body, two subcommands
/// (`Pass 157.0`).
///
/// `title` is `Some` to rename and `None` to delete. One function because the
/// two share every step except the verb they call: resolving `n` to an object
/// id is the whole of the work, and duplicating that is duplicating the only
/// part that can be wrong.
///
/// # Why `n` rather than an object number
///
/// Because `list-outline` prints `n=`, and a CLI whose identifier does not
/// appear in the output of the command that lists things is a CLI you cannot
/// script without a PDF parser. The object id is an implementation detail of
/// the file; `n` is a fact about what pdfcer showed you.
pub(crate) fn cmd_edit_bookmark(
    input: &Path,
    n: usize,
    title: Option<&str>,
    output: &Path,
    mode: SaveMode,
) -> u8 {
    if n == 0 {
        eprintln!(
            "pdfcer: {}: --n is 1-based, matching `list-outline`'s own numbering",
            input.display()
        );
        return exit::RUNTIME_ERROR;
    }
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    // Resolve `n` the same way `list-outline` numbers: document order, every
    // level, which is exactly what `Outline::flatten` yields.
    let (item_id, item_title) = {
        let outline = pdfcer_core::outline::read_outline(&session.graph());
        let flat = outline.flatten();
        let Some(item) = flat.get(n - 1) else {
            eprintln!(
                "pdfcer: {}: no bookmark {n} — the document has {}",
                input.display(),
                flat.len()
            );
            return exit::RUNTIME_ERROR;
        };
        (item.id, item.title.clone())
    };

    let removed = match title {
        Some(t) => {
            if let Err(err) = session.set_outline_title(item_id, t) {
                return report_edit_error(input, &err);
            }
            None
        }
        None => match session.delete_outline_item(item_id) {
            Ok(count) => Some(count),
            Err(err) => return report_edit_error(input, &err),
        },
    };

    // The disclosure: a deleted bookmark takes its children, and the operator
    // named ONE. The invocation is the commit here, so this is the only
    // chance to say how much went.
    if let Some(count) = removed
        && count > 1
    {
        eprintln!(
            "pdfcer: {}: {count} outline objects were removed, not 1 — deleting a bookmark takes everything beneath it, as Acrobat does. Promoting its children would have spliced them into the level above.",
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

    match title {
        Some(t) => println!(
            "rename-bookmark {} n={n} {:?} -> {:?} -> {}",
            input.display(),
            item_title,
            t,
            output.display()
        ),
        None => println!(
            "delete-bookmark {} n={n} {:?} removed={} -> {}",
            input.display(),
            item_title,
            removed.unwrap_or(0),
            output.display()
        ),
    }
    finish_edit(input, &saved)
}

/// The `n`th item (1-based) in depth-first document order — the order
/// `list-outline` prints and `--under` indexes.
///
/// Written as an explicit stack rather than a recursive walk with a counter,
/// because the recursive form needs the counter threaded by `&mut` through
/// every frame and a missed increment produces an off-by-one that only shows
/// on nested trees. The reader has already applied its own depth cap, so the
/// tree handed here is finite.
pub(crate) fn nth_outline_item(
    items: &[pdfcer_core::outline::OutlineItem],
    n: usize,
) -> Option<pdfcer_core::object::ObjId> {
    let mut seen = 0usize;
    let mut stack: Vec<&pdfcer_core::outline::OutlineItem> = items.iter().rev().collect();
    while let Some(item) = stack.pop() {
        seen += 1;
        if seen == n {
            return Some(item.id);
        }
        stack.extend(item.children.iter().rev());
    }
    None
}

/// How many items the whole tree holds, for the refusal message and the
/// success line. Shares [`nth_outline_item`]'s traversal shape deliberately:
/// a count that disagreed with the indexing would make the refusal message
/// name a range that does not work.
pub(crate) fn count_outline_items(items: &[pdfcer_core::outline::OutlineItem]) -> usize {
    let mut n = 0usize;
    let mut stack: Vec<&pdfcer_core::outline::OutlineItem> = items.iter().collect();
    while let Some(item) = stack.pop() {
        n += 1;
        stack.extend(item.children.iter());
    }
    n
}

/// `move-bookmark` — reorder or re-parent one bookmark (`Pass 161.0`).
///
/// # Why the four destination flags collapse to one enum here
///
/// `clap`'s `group = "destination"` makes `--before`, `--after`, `--under` and
/// `--to-top-level` mutually exclusive at parse time, so this function never
/// sees two of them. It can still see **none**, which is the one combination
/// the group cannot forbid without making a destination mandatory in a way
/// that reads badly in `--help`, so it is refused here by name.
///
/// `--first` is deliberately NOT in the group: it modifies `--under` and
/// `--to-top-level` rather than competing with them, and it is silently
/// meaningless with `--before`/`--after` — which is refused rather than
/// ignored, because a flag that does nothing is a flag whose author believed
/// it did something.
///
/// # The disclosure
///
/// The invocation is the commit in `pdfcer` — no session, no undo — so
/// everything the core worked out is printed on the way past (project rule 4,
/// `pdfcer` half). That includes the case where **nothing happened**:
/// `moved=0` is printed rather than a silent success, because "the bookmark is
/// already there" and "the command did not run" look identical otherwise.
///
/// A move into a collapsed parent reduces the counts above it even though
/// nothing was deleted. That is correct and surprising, so it gets its own
/// sentence on stderr rather than being left for the operator to discover from
/// a viewer's panel.
// Nine parameters, and they are nine because the SUBCOMMAND has nine flags:
// this function's shape is `clap`'s, not a design choice. Bundling them into
// a struct would add a type whose only job is to be destructured one line
// later, and would put the destination flags one indirection away from the
// mutual-exclusion check that is the whole reason they are separate.
#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_move_bookmark(
    input: &Path,
    n: usize,
    before: Option<usize>,
    after: Option<usize>,
    under: Option<usize>,
    to_top_level: bool,
    first: bool,
    output: &Path,
    mode: SaveMode,
) -> u8 {
    if n == 0 {
        eprintln!(
            "pdfcer: {}: --n is 1-based, matching `list-outline`'s own numbering",
            input.display()
        );
        return exit::RUNTIME_ERROR;
    }
    if before.is_none() && after.is_none() && under.is_none() && !to_top_level {
        eprintln!(
            "pdfcer: {}: a move needs a destination — one of --before N, --after N, --under N or --to-top-level",
            input.display()
        );
        return exit::RUNTIME_ERROR;
    }
    if first && (before.is_some() || after.is_some()) {
        eprintln!(
            "pdfcer: {}: --first modifies --under and --to-top-level; with --before or --after the position is already exact",
            input.display()
        );
        return exit::RUNTIME_ERROR;
    }
    for (flag, value) in [("--before", before), ("--after", after), ("--under", under)] {
        if value == Some(0) {
            eprintln!(
                "pdfcer: {}: {flag} is 1-based, matching `list-outline`'s own numbering",
                input.display()
            );
            return exit::RUNTIME_ERROR;
        }
    }

    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    // Resolve every `n=` against ONE reading of the outline, before anything
    // is edited. Resolving the anchor after the move would index a tree that
    // has already changed shape under it.
    let (item_id, item_title, anchor_id) = {
        let outline = pdfcer_core::outline::read_outline(&session.graph());
        let total = count_outline_items(&outline.items);
        let resolve = |which: usize| nth_outline_item(&outline.items, which);
        let Some(item_id) = resolve(n) else {
            eprintln!(
                "pdfcer: {}: no bookmark {n} — the document has {total}",
                input.display()
            );
            return exit::RUNTIME_ERROR;
        };
        let title = outline
            .flatten()
            .get(n - 1)
            .map_or_else(String::new, |i| i.title.clone());
        let anchor = match before.or(after).or(under) {
            None => None,
            Some(which) => match resolve(which) {
                Some(id) => Some(id),
                None => {
                    eprintln!(
                        "pdfcer: {}: no bookmark {which} — the document has {total}",
                        input.display()
                    );
                    return exit::RUNTIME_ERROR;
                }
            },
        };
        (item_id, title, anchor)
    };

    let placement = match (before, after, under, to_top_level) {
        (Some(_), _, _, _) => pdfcer_core::edit::OutlinePlacement::Before {
            sibling: anchor_id.expect("--before resolved above"),
        },
        (_, Some(_), _, _) => pdfcer_core::edit::OutlinePlacement::After {
            sibling: anchor_id.expect("--after resolved above"),
        },
        (_, _, Some(_), _) if first => {
            pdfcer_core::edit::OutlinePlacement::FirstChild { parent: anchor_id }
        }
        (_, _, Some(_), _) => pdfcer_core::edit::OutlinePlacement::LastChild { parent: anchor_id },
        (_, _, _, true) if first => {
            pdfcer_core::edit::OutlinePlacement::FirstChild { parent: None }
        }
        (_, _, _, true) => pdfcer_core::edit::OutlinePlacement::LastChild { parent: None },
        // Unreachable: the four-flag guard above returns before this point.
        _ => unreachable!("a destination was verified present"),
    };

    let report = match session.move_outline_item(item_id, placement) {
        Ok(r) => r,
        // The cycle refusal is re-phrased rather than passed through.
        //
        // The core's message names OBJECT IDS, which is right for the core —
        // its caller passed object ids. But this command's operator never saw
        // one: they typed `--n 1 --under 2`, and an error that answers with
        // "bookmark 9 0 cannot be moved under 11 0" is a message about
        // identifiers that appear nowhere in `list-outline`'s output. Every
        // other refusal in this function speaks `n=`; this one has to as well,
        // or the CLI has two vocabularies and the operator has to learn the
        // one pdfcer uses internally to act on the failure.
        Err(pdfcer_core::edit::EditError::OutlineMoveIntoOwnSubtree { .. }) => {
            let target = before.or(after).or(under);
            eprintln!(
                "pdfcer: {}: bookmark {n} cannot be moved {} — that would put it inside its own subtree, making the outline's /Parent chain a cycle. Move it to a bookmark that is not beneath it, or promote it with --to-top-level first.",
                input.display(),
                match target {
                    Some(t) => format!("under bookmark {t}"),
                    None => "there".to_string(),
                }
            );
            return exit::RUNTIME_ERROR;
        }
        Err(err) => return report_edit_error(input, &err),
    };

    if report.moved && report.reparented && report.visible_items > 1 {
        eprintln!(
            "pdfcer: {}: {} outline items moved, not 1 — a bookmark takes everything beneath it, as Acrobat does.",
            input.display(),
            report.visible_items
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
        "move-bookmark {} n={n} {:?} moved={} reparented={} visible={} -> {}",
        input.display(),
        item_title,
        u32::from(report.moved),
        u32::from(report.reparented),
        report.visible_items,
        output.display()
    );
    if !report.moved {
        eprintln!(
            "pdfcer: {}: the bookmark is already in that position — nothing was written",
            input.display()
        );
    }
    finish_edit(input, &saved)
}

/// `set-bookmark-open` — expand or collapse one bookmark (`Pass 161.0`).
///
/// See [`cmd_move_bookmark`] for why this is a separate verb rather than a
/// flag on the move: the two answers to "what happens to a collapsed
/// destination" are both defensible, so both ship, and composing them is one
/// extra invocation rather than a boolean buried in an unrelated command.
pub(crate) fn cmd_set_bookmark_open(
    input: &Path,
    n: usize,
    collapse: bool,
    output: &Path,
    mode: SaveMode,
) -> u8 {
    if n == 0 {
        eprintln!(
            "pdfcer: {}: --n is 1-based, matching `list-outline`'s own numbering",
            input.display()
        );
        return exit::RUNTIME_ERROR;
    }
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let (item_id, item_title) = {
        let outline = pdfcer_core::outline::read_outline(&session.graph());
        let flat = outline.flatten();
        let Some(item) = flat.get(n - 1) else {
            eprintln!(
                "pdfcer: {}: no bookmark {n} — the document has {}",
                input.display(),
                flat.len()
            );
            return exit::RUNTIME_ERROR;
        };
        (item.id, item.title.clone())
    };

    let changed = match session.set_outline_open(item_id, !collapse) {
        Ok(c) => c,
        Err(err) => return report_edit_error(input, &err),
    };

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
        "set-bookmark-open {} n={n} {:?} open={} changed={} -> {}",
        input.display(),
        item_title,
        u32::from(!collapse),
        u32::from(changed),
        output.display()
    );
    if !changed {
        eprintln!(
            "pdfcer: {}: that bookmark has no children, or was already in that state — nothing was written",
            input.display()
        );
    }
    finish_edit(input, &saved)
}

/// `adopt-widget` — register an existing widget annotation as a form field
/// (§12.7.3).
///
/// # Why this is addressed by page + index rather than by name
///
/// Because the widgets it exists for **have no name** — that is the whole
/// condition. An orphan left by `insert-pages` is not in `/AcroForm`, so
/// `list-fields` cannot show it and no name-based selector can reach it.
/// `list-annotations` can, and its `page=`/`index=` pair is what this takes,
/// so the two compose the way `list-outline` and `add-bookmark` do.
///
/// # What it prints
///
/// `renamed=` and `acroform_created=` are reported because **neither is
/// visible in the result**. A widget whose `/T` was changed looks exactly
/// like one that always had that name, and a fresh `/AcroForm` looks exactly
/// like an old one — so an operator re-reading the file afterwards cannot
/// discover either. This line is the only disclosure.
#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_adopt_widget(
    input: &Path,
    page: usize,
    index: usize,
    name: Option<&str>,
    dry_run: bool,
    output: Option<&Path>,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    // Resolved inside a block so the session borrow ends before the mutable
    // call below.
    let widget_id = {
        let slots = match session.page_slots() {
            Ok(slots) => slots,
            Err(err) => {
                eprintln!("pdfcer: {}: {err}", input.display());
                return exit::RUNTIME_ERROR;
            }
        };
        let Some(slot) = slots.get(page.saturating_sub(1)) else {
            eprintln!(
                "pdfcer: {}: no page {page} — the document has {} page(s)",
                input.display(),
                slots.len()
            );
            return exit::RUNTIME_ERROR;
        };
        let annots = pdfcer_core::annot::page_annotations(&session.graph(), slot.id);
        let Some(annot) = annots.get(index) else {
            eprintln!(
                "pdfcer: {}: page {page} has no annotation at index {index} — it has {}",
                input.display(),
                annots.len(),
            );
            return exit::RUNTIME_ERROR;
        };
        let Some(id) = annot.id else {
            eprintln!(
                "pdfcer: {}: page {page} index {index} is a direct dictionary inside /Annots, not an indirect object — a form field must be an indirect object to be referenced from /AcroForm",
                input.display()
            );
            return exit::EDIT_REFUSED;
        };
        id
    };

    // `--dry-run` answers BEFORE the press, which is the whole point.
    //
    // `pdfcer-gui` asked for `adopt_preview` because a widget's two shapes are
    // indistinguishable from the outside: one adopts losslessly, the other
    // refuses and can only be made into a NEW empty field. The same is true
    // at a shell prompt, and worse — there is no row to grey out, so without
    // this the operator's only way to find out is to write a file.
    if dry_run {
        match session.adopt_preview(widget_id, name) {
            Ok(o) => {
                println!(
                    "adopt-widget {} page {page} index {index} DRY RUN; would_register={:?} type={} renamed={} acroform_created={}",
                    input.display(),
                    o.name,
                    o.field_type.as_deref().unwrap_or("none"),
                    u32::from(o.renamed),
                    u32::from(o.acroform_created),
                );
                if o.field_type.is_none() {
                    eprintln!(
                        "pdfcer: {}: {:?} would have no field type (/FT) and would be top-level, so it would inherit none — registering it would succeed and it still would not be fillable",
                        input.display(),
                        o.name,
                    );
                }
                return exit::SUCCESS;
            }
            Err(err) => return report_edit_error(input, &err),
        }
    }
    let Some(output) = output else {
        // Unreachable: clap's `required_unless_present` enforces it. Handled
        // rather than unwrapped so a future flag change cannot turn a
        // missing path into a panic in front of an operator.
        eprintln!(
            "pdfcer: {}: --output is required unless --dry-run",
            input.display()
        );
        return exit::EDIT_REFUSED;
    };

    let outcome = match session.adopt_widget(widget_id, name) {
        Ok(outcome) => outcome,
        Err(err) => return report_edit_error(input, &err),
    };

    let saved = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        verify_undo,
    ) {
        Ok(saved) => saved,
        Err(code) => return code,
    };

    let r = &saved.report;
    println!(
        "adopt-widget {} page {page} index {index} mode={} -> {}; \
field={:?} type={} renamed={} acroform_created={} changed={} objects={} \
verbatim={} reserialized={} appended={} out_bytes={} undo_verified={} \
undo_identical={} delinearized={}",
        input.display(),
        mode.name(),
        output.display(),
        outcome.name,
        outcome.field_type.as_deref().unwrap_or("none"),
        u32::from(outcome.renamed),
        u32::from(outcome.acroform_created),
        saved.changed,
        r.objects_written,
        r.objects_verbatim,
        r.objects_reserialized,
        r.bytes_appended,
        r.bytes_written,
        u32::from(saved.undo_verified),
        u32::from(saved.undo_identical),
        u32::from(r.delinearized),
    );
    if outcome.field_type.is_none() {
        // §12.7.3.1 makes `/FT` inheritable, and a top-level field has
        // nothing left to inherit from — so this field has no type at all
        // and no viewer can decide how to render or fill it. Said out loud
        // rather than left in a `type=none` token nobody reads.
        eprintln!(
            "pdfcer: {}: {:?} has no field type (/FT) and is now top-level, so it inherits none — viewers will not know how to fill it",
            input.display(),
            outcome.name,
        );
    }
    finish_edit(input, &saved)
}

/// `add-named-dest` — define a named destination (§12.3.2.3).
///
/// # `--name` is a `String` here and bytes in the engine, deliberately
///
/// §7.9.6 imposes no encoding on name-tree keys, and
/// `EditSession::add_named_destination` therefore takes `&[u8]` so a caller
/// carrying a key read out of another document can pass it through
/// untouched. A **command line** cannot carry arbitrary bytes portably, so
/// this shell takes text and hands over its UTF-8. That is a narrowing, and
/// it is the right one to make here rather than in the engine: a key typed
/// at a shell prompt is text by construction, while a key copied between
/// documents is not, and only the engine sees the second case.
#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_add_named_dest(
    input: &Path,
    name: &str,
    page: u32,
    top: Option<f64>,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let Some(index) = page.checked_sub(1).map(|i| i as usize) else {
        eprintln!(
            "pdfcer: {}: --page is 1-based; 0 is not a page",
            input.display()
        );
        return exit::EDIT_REFUSED;
    };
    // `--page` alone means /Fit rather than /XYZ with nulls, for the same
    // reason as `add-bookmark`: /XYZ null null null means "keep whatever the
    // viewer is showing", which makes a scripted destination depend on where
    // the reader happened to be.
    let view = match top {
        Some(y) => pdfcer_core::outline::DestView::Xyz {
            left: None,
            top: Some(y),
            zoom: None,
        },
        None => pdfcer_core::outline::DestView::Fit,
    };
    if let Err(err) = session.add_named_destination(
        name.as_bytes(),
        pdfcer_core::outline::Destination::Page {
            page_index: index,
            view,
        },
    ) {
        return report_edit_error(input, &err);
    }

    // Read BEFORE the save, not after.
    //
    // `save_edited` with `--verify-undo` runs `while session.undo().is_some()
    // {}` and never redoes, so the session it hands back holds the document as
    // it was BEFORE any edit. Any state read from it afterwards is pre-edit
    // state. This command reported `names=0` immediately after successfully
    // defining a destination — exactly what a document with none would print,
    // which is why it survived until the command's own output was read with
    // and without the flag side by side (R174).
    let names =
        pdfcer_core::pageops::references::DestinationResolver::new(&session.graph()).named_count();

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
        "add-named-dest {} name={name:?} page={page} mode={} -> {}; \
names={names} changed={} objects={} verbatim={} reserialized={} appended={} \
out_bytes={} undo_verified={} undo_identical={} delinearized={}",
        input.display(),
        mode.name(),
        output.display(),
        outcome.changed,
        r.objects_written,
        r.objects_verbatim,
        r.objects_reserialized,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
        u32::from(r.delinearized),
    );
    finish_edit(input, &outcome)
}

/// `merge-document` — merge a whole document into the input, incrementally.
///
/// # Why this exists beside `insert-pages`
///
/// `insert-pages` calls `pageops::insert`, which assembles a **new**
/// document. That is fine for a one-shot CLI invocation and fatal for an
/// editor, which is why `pdfcer-gui` could not wire Merge to it. This calls
/// `EditSession::merge_document`, the incremental verb built for them —
/// exposed here too because a CLI operator benefits from the same property
/// in a different way: the output is an **incremental save**, so an existing
/// signature over the input's byte range stays intact.
#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_merge_document(
    input: &Path,
    source_path: &Path,
    before: Option<usize>,
    after: Option<usize>,
    at_start: bool,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    let (source_bytes, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let merged = match std::fs::read(source_path) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", source_path.display());
            return exit::IO_ERROR;
        }
    };
    let merged = match pdfcer_core::document::Document::from_bytes(merged) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", source_path.display());
            return exit::NOT_A_PDF;
        }
    };

    // 1-based on the command line, 0-based in the engine. `checked_sub`
    // handles `--before 0` without wrapping.
    let position = if at_start {
        pdfcer_core::pageops::InsertPosition::Start
    } else if let Some(n) = before {
        match n.checked_sub(1) {
            Some(i) => pdfcer_core::pageops::InsertPosition::Before(i),
            None => {
                eprintln!(
                    "pdfcer: {}: --before is 1-based; 0 is not a page (use --at-start)",
                    input.display()
                );
                return exit::EDIT_REFUSED;
            }
        }
    } else if let Some(n) = after {
        match n.checked_sub(1) {
            Some(i) => pdfcer_core::pageops::InsertPosition::After(i),
            None => {
                eprintln!(
                    "pdfcer: {}: --after is 1-based; 0 is not a page (use --at-start)",
                    input.display()
                );
                return exit::EDIT_REFUSED;
            }
        }
    } else {
        pdfcer_core::pageops::InsertPosition::End
    };

    let outcome = match session.merge_document(&merged.view(), position) {
        Ok(outcome) => outcome,
        Err(err) => return report_edit_error(input, &err),
    };

    let saved = match save_edited(
        &mut session,
        &source_bytes,
        output,
        mode,
        ProducerArg::Preserve,
        verify_undo,
    ) {
        Ok(saved) => saved,
        Err(code) => return code,
    };

    let r = &saved.report;
    println!(
        "merge-document {} + {} mode={} -> {}; \
pages={} fields={} renamed={} acroform_created={} dests={} dests_renamed={} \
outline_items={} changed={} objects={} \
verbatim={} reserialized={} appended={} out_bytes={} undo_verified={} \
undo_identical={} delinearized={}",
        input.display(),
        source_path.display(),
        mode.name(),
        output.display(),
        outcome.pages_merged,
        outcome.fields_merged,
        outcome.fields_renamed,
        u32::from(outcome.acroform_created),
        // `Pass 106.2`. The three counts `Pass 106.1` added to `MergeOutcome`
        // and that this line did not print. A shell that computes a
        // disclosure and does not emit it has the same effect as one that
        // never computed it — and in the CLI the invocation IS the commit
        // (rule 11), so there is no later screen to find it on.
        outcome.named_destinations_carried,
        outcome.named_destinations_renamed,
        outcome.outline_items_carried,
        saved.changed,
        r.objects_written,
        r.objects_verbatim,
        r.objects_reserialized,
        r.bytes_appended,
        r.bytes_written,
        u32::from(saved.undo_verified),
        u32::from(saved.undo_identical),
        u32::from(r.delinearized),
    );
    if outcome.named_destinations_renamed > 0 {
        // Promoted out of the token line for the same reason `fields_renamed`
        // is, plus one the field's own doc comment gives and nothing else
        // surfaces: pdfcer rewrites the bookmarks it CARRIED to the new keys,
        // but cannot rewrite a link it did not copy — and it copies only what
        // the merged pages reach. So a `/GoToR` in a THIRD document pointing
        // at the old key now silently resolves to this document's own
        // destination instead of the source's. That is a cross-file breakage
        // an operator has no other way to learn about.
        eprintln!(
            "pdfcer: {}: {} named destination(s) were renamed because the key was already defined here; a link from a third document to the old key now resolves to this document's own destination",
            input.display(),
            outcome.named_destinations_renamed,
        );
    }
    if outcome.fields_renamed > 0 {
        // Named rather than left in a token, because a renamed field breaks
        // any script, FDF or calculation keyed on the old name -- and the
        // operator has no other way to learn it happened.
        eprintln!(
            "pdfcer: {}: {} field(s) were renamed because the name was already taken; scripts or FDF keyed on the old names will no longer match",
            input.display(),
            outcome.fields_renamed,
        );
    }
    finish_edit(input, &saved)
}

/// `list-outline` — the document's bookmarks, as a tree.
///
/// # Why the indentation is real output and not decoration
///
/// An outline's SHAPE is its meaning: "Chapter 3" nested under "Part II"
/// says something a flat list of titles does not. So the level is both
/// rendered as indentation for a person and printed as `level=` for a
/// script, rather than one or the other.
///
/// # `read_outline`, not `parse_outline`
///
/// The core module offers both. `parse_outline` is the thin wrapper that
/// returns items alone and **silently discards the diagnostics** —
/// including "this tree was truncated because it contained a cycle".
/// A command that reported a partial outline as if it were the whole
/// thing would be making a claim about the document it cannot support.
pub(crate) fn cmd_list_outline(input: &Path, flat: bool, json: bool) -> u8 {
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let session = pdfcer_core::edit::EditSession::new(doc);
    let outline = pdfcer_core::outline::read_outline(&session.graph());

    // ---- JSON: the machine-readable shape, for a folder of files ----
    //
    // Emitted from the SAME `Outline` the text branch walks, so the two
    // cannot disagree about what the document contains. Only the rendering
    // differs, which is the whole reason this is a flag and not a second
    // command with a second reader.
    if json {
        fn emit_json(
            items: &[pdfcer_core::outline::OutlineItem],
            n: &mut usize,
            out: &mut Vec<String>,
        ) {
            for it in items {
                *n += 1;
                // `target_file` is the operator-facing answer: which OTHER
                // file this bookmark opens. Both action types that name one
                // are folded together here — a `/Launch` and a `/GoToR`
                // answer the same question, and a script asking "what does
                // this ToC point at" should not have to know which spelling
                // the producer chose.
                let (kind, file, page) = match &it.destination {
                    Some(Destination::Page { page_index, .. }) => {
                        ("page".to_owned(), None, Some(*page_index))
                    }
                    Some(Destination::UnmappedPage { .. }) => ("unmapped".to_owned(), None, None),
                    Some(Destination::Named { .. }) => ("named".to_owned(), None, None),
                    Some(Destination::Remote { file, .. }) => (
                        "remote".to_owned(),
                        file.as_ref()
                            .map(|b| String::from_utf8_lossy(b).into_owned()),
                        None,
                    ),
                    Some(Destination::NonNavigation { action, file }) => (
                        action.as_ref().map_or_else(
                            || "action".to_owned(),
                            |a| String::from_utf8_lossy(a.as_bytes()).into_owned(),
                        ),
                        file.as_ref()
                            .map(|b| String::from_utf8_lossy(b).into_owned()),
                        None,
                    ),
                    // `Destination` is `#[non_exhaustive]`: a variant added
                    // later must land here visibly rather than be silently
                    // reported as one of the shapes above.
                    Some(_) => ("other".to_owned(), None, None),
                    None => ("none".to_owned(), None, None),
                };
                let mut fields = vec![
                    format!("\"n\":{n}"),
                    format!("\"obj\":{}", it.id.num),
                    format!("\"level\":{}", it.level),
                    format!("\"title\":\"{}\"", json_escape(&it.title)),
                    format!("\"kind\":\"{}\"", json_escape(&kind)),
                ];
                if let Some(f) = file.as_deref() {
                    fields.push(format!("\"file\":\"{}\"", json_escape(f)));
                }
                if let Some(pg) = page {
                    fields.push(format!("\"page\":{}", pg + 1));
                }
                out.push(format!("  {{{}}}", fields.join(",")));
                emit_json(&it.children, n, out);
            }
        }
        let mut rows = Vec::new();
        let mut n = 0usize;
        emit_json(&outline.items, &mut n, &mut rows);
        println!("[");
        println!("{}", rows.join(",\n"));
        println!("]");
        return exit::SUCCESS;
    }

    // `n` is threaded through rather than counted per-level so it is the
    // DOCUMENT-order index, which is what `add-bookmark --under` takes. A
    // per-level counter would print 1,2,1,2 and silently mean something
    // else on a nested tree.
    fn emit(items: &[pdfcer_core::outline::OutlineItem], flat: bool, n: &mut usize) -> usize {
        let mut shown = 0;
        for it in items {
            *n += 1;
            shown += 1;
            // `describe_destination`, NOT `{d:?}` (`Pass 258.2`). The
            // Rust `Debug` form printed a `/GoToR`'s filename as a decimal
            // BYTE ARRAY — `file: Some([84, 101, ...])` — which is the
            // right information rendered unusably, and an operator asking
            // "which file does this bookmark open" could not read it.
            // `list-links` had the readable renderer all along; the two
            // commands answer the same question about the same key and
            // now answer it the same way.
            let dest = match &it.destination {
                Some(d) => describe_destination(d),
                // Distinguished from a destination pdfcer could not
                // resolve: an item with no destination at all is a
                // heading, which is legal and common.
                None => "dest=-".to_owned(),
            };
            let indent = if flat {
                String::new()
            } else {
                "  ".repeat(it.level)
            };
            // `obj=` is the OBJECT NUMBER, added with `Pass 172.0`, and it
            // is not decoration: `bookmark-copy --item N` takes it, and `n=`
            // is a sequence counter that means nothing to any other verb.
            // Without it the bookmark clipboard was documented, correct and
            // unreachable from the one command that lists bookmarks.
            println!(
                "{indent}bookmark n={n} obj={} level={} open={} title={:?} {dest}",
                it.id.num,
                it.level,
                u32::from(it.open),
                it.title,
            );
            shown += emit(&it.children, flat, n);
        }
        shown
    }
    let shown = emit(&outline.items, flat, &mut 0);

    // The diagnostics are not a footnote. A truncated tree looks exactly
    // like a short one from the outside, and only this line distinguishes
    // them.
    //
    // Reported as only the NON-ZERO counters. The struct carries twenty-odd
    // fields and on a healthy document every one is zero — dumping them all
    // put the two that matter inside a wall of `foo: 0`, which is how a real
    // warning gets skimmed past. The cycle fixture made it obvious:
    // `cycles_broken: 3` was in there, and invisible. Found by reading the
    // command's own output (R174).
    let d = &outline.diagnostics;
    let mut notes: Vec<String> = Vec::new();
    if d.item_budget_exhausted {
        notes.push("item_budget_exhausted".to_owned());
    }
    if d.root_count_disagreement {
        notes.push("root_count_disagreement".to_owned());
    }
    for (c, name) in [
        (d.depth_truncations, "depth_truncations"),
        (d.cycles_broken, "cycles_broken"),
        (d.unreadable_items, "unreadable_items"),
        (d.titles_unreadable, "titles_unreadable"),
        (d.titles_inexact, "titles_inexact"),
        (d.unmapped_pages, "unmapped_pages"),
        (d.unresolved_names, "unresolved_names"),
        (d.count_disagreements, "count_disagreements"),
        (d.unknown_views, "unknown_views"),
        (d.malformed_views, "malformed_views"),
        (d.cross_namespace_resolutions, "cross_namespace_resolutions"),
        (d.non_reference_links, "non_reference_links"),
        (d.unreadable_actions, "unreadable_actions"),
        (
            d.dest_and_action_both_present,
            "dest_and_action_both_present",
        ),
    ] {
        if c > 0 {
            notes.push(format!("{name}={c}"));
        }
    }
    if let Some(e) = &d.page_tree_error {
        notes.push(format!("page_tree_error={e:?}"));
    }
    let warnings = if notes.is_empty() {
        "clean".to_owned()
    } else {
        notes.join(" ")
    };
    println!(
        "list-outline {} bookmarks={shown} max_depth={} {warnings}",
        input.display(),
        d.max_depth,
    );
    exit::SUCCESS
}

/// Borrowed argument bundle for [`cmd_bookmark_copy`].
pub(crate) struct BookmarkCopyArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) item: u32,
    pub(crate) clip: &'a Path,
    pub(crate) cut: Option<&'a Path>,
    pub(crate) mode: SaveMode,
}

/// `bookmark-copy` — a bookmark subtree onto a clipboard file.
pub(crate) fn cmd_bookmark_copy(args: &BookmarkCopyArgs<'_>) -> u8 {
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let id = pdfcer_core::object::ObjId::new(args.item, 0);
    let clip = if args.cut.is_some() {
        match session.cut_outline_item(id) {
            Ok(clip) => clip,
            Err(err) => return report_edit_error(args.input, &err),
        }
    } else {
        match session.copy_outline_item(id) {
            Ok(clip) => clip,
            Err(err) => return report_edit_error(args.input, &err),
        }
    };
    let bytes = clip.to_bytes();
    if let Err(err) = std::fs::write(args.clip, &bytes) {
        eprintln!("pdfcer: {}: {err}", args.clip.display());
        return exit::IO_ERROR;
    }
    println!(
        "{} {} item={} bookmarks={} deepest_page={} -> {} ({} bytes)",
        if args.cut.is_some() {
            "bookmark-cut"
        } else {
            "bookmark-copy"
        },
        args.input.display(),
        args.item,
        clip.len(),
        clip.deepest_page()
            .map_or_else(|| "-".to_owned(), |p| (p + 1).to_string()),
        args.clip.display(),
        bytes.len(),
    );

    let Some(cut_output) = args.cut else {
        return exit::SUCCESS;
    };
    let outcome = match save_edited(
        &mut session,
        &source,
        cut_output,
        args.mode,
        ProducerArg::Preserve,
        false,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    println!(
        "  cut=1 cut_out={} mode={} changed={}",
        cut_output.display(),
        args.mode.name(),
        outcome.changed,
    );
    finish_edit(args.input, &outcome)
}

/// Borrowed argument bundle for [`cmd_bookmark_paste`].
pub(crate) struct BookmarkPasteArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) clip: &'a Path,
    pub(crate) under: Option<u32>,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
}

/// `bookmark-paste` — a bookmark subtree from a clipboard file.
pub(crate) fn cmd_bookmark_paste(args: &BookmarkPasteArgs<'_>) -> u8 {
    let bytes = match std::fs::read(args.clip) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", args.clip.display());
            return exit::IO_ERROR;
        }
    };
    let clip = match pdfcer_core::outline::OutlineClip::from_bytes(&bytes) {
        Ok(clip) => clip,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", args.clip.display());
            return exit::EDIT_REFUSED;
        }
    };
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let placement = pdfcer_core::edit::OutlinePlacement::LastChild {
        parent: args.under.map(|n| pdfcer_core::object::ObjId::new(n, 0)),
    };
    let pasted = match session.paste_outline_item(&clip, placement) {
        Ok(outcome) => outcome,
        Err(err) => return report_edit_error(args.input, &err),
    };
    if pasted.destinations_dropped > 0 {
        eprintln!(
            "pdfcer: {}: {} bookmark(s) arrived WITHOUT their destination -- it named a page this document does not have. Not clamped to the last page: a bookmark that navigates confidently to the wrong place is worse than one that plainly does not navigate. They still show, and clicking them does nothing.",
            args.input.display(),
            pasted.destinations_dropped
        );
    }
    let outcome = match save_edited(
        &mut session,
        &source,
        args.output,
        args.mode,
        ProducerArg::Preserve,
        false,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    println!(
        "bookmark-paste {} clip={} under={} bookmarks={} destinations_dropped={} mode={} -> {}; changed={}",
        args.input.display(),
        args.clip.display(),
        args.under
            .map_or_else(|| "top-level".to_owned(), |n| n.to_string()),
        pasted.items_pasted,
        pasted.destinations_dropped,
        args.mode.name(),
        args.output.display(),
        outcome.changed,
    );
    finish_edit(args.input, &outcome)
}
