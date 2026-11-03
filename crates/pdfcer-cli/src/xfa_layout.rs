//! `xfa-layout`: print the pages pdfcer computes from an XFA template.

use super::*;

use pdfcer_core::xfa::{
    ContainerKind, LayoutItem, LayoutOptions, LayoutPage, LayoutWarning, MergeOptions, MergedForm,
    XfaLayout,
};

/// `xfa-layout --negative-coordinates` (gap G-L1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum NegativeCoordinatesArg {
    /// Keep the template's geometry, even off the page.
    #[default]
    Keep,
    /// Clamp a negative coordinate to zero.
    Clamp,
}

/// `xfa-layout --nested-overflow` (gap G-L21).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum NestedOverflowArg {
    /// Only the innermost split subform lays a leader and trailer.
    #[default]
    Innermost,
    /// Every split subform with its own leader or trailer lays it.
    EachAsserting,
}

/// `xfa-layout --page-area-keep` (gap G-L12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum PageAreaKeepArg {
    /// Keep the pair on one page, as XFA 3.3 defines it.
    #[default]
    Spec,
    /// Ignore it, as Adobe's processor does.
    Adobe,
}

/// `xfa-layout --odd-rotate` (gap G-L29).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum OddRotateArg {
    /// No rotation.
    #[default]
    Ignore,
    /// The nearest multiple of 90 degrees.
    Nearest,
}

/// `xfa-layout --tiled-split` (XFA 3.3 pp.281-282).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum TiledSplitArg {
    /// An lr-tb or rl-tb subform splits between its rows.
    #[default]
    Split,
    /// Only tb subforms split; a tiled one moves whole to the next page.
    TbOnly,
}

/// `xfa-layout --table-cell-height` (decision 212).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum TableCellHeightArg {
    /// A flowing cell in a table row is as tall as its content.
    #[default]
    Content,
    /// A cell keeps the height its template declares.
    Declared,
}

/// `xfa-layout --scope-search` (gap G-SOM3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum ScopeSearchArg {
    /// The current data group's children, then each enclosing group's.
    #[default]
    Upward,
    /// The current data group's children only.
    Children,
}

/// `xfa-layout --bare-ref` (gap G-SOM8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum BareRefArg {
    /// Ignore a ref without match="dataRef".
    #[default]
    Ignore,
    /// Bind it as a data reference.
    DataRef,
}

/// `xfa-layout --data-attributes` (gap G-SOM6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum DataAttributesArg {
    /// Attributes are data values.
    #[default]
    Load,
    /// Attributes are not data.
    Ignore,
}

/// `xfa-layout --global-scope` (gap G-B9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum GlobalScopeArg {
    /// The first same-named value anywhere in the data.
    #[default]
    Document,
    /// Only values outside every record.
    Records,
}

/// `xfa-layout --global-exclgroup` (gap G-B10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum GlobalExclGroupArg {
    /// An exclusion group shares its value like a field.
    #[default]
    Honour,
    /// An exclusion group binds as match="once".
    AsOnce,
}

/// `xfa-layout --bind-items-repeat` (gap G-BI1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum BindItemsRepeatArg {
    /// The last bindItems wins.
    #[default]
    Last,
    /// The first bindItems wins.
    First,
}

/// `xfa-layout --bind-items-missing` (gap G-BI4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum BindItemsMissingArg {
    /// An item whose value or label is missing gets an empty string.
    #[default]
    Empty,
    /// An item whose value or label is missing is left out.
    Skip,
}

/// `xfa-layout --bind-items-unbound` (gap G-BI5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum BindItemsUnboundArg {
    /// Resolve from the enclosing data scope.
    #[default]
    Scope,
    /// Keep the template's own items.
    Ignore,
}

/// `xfa-layout --choice-instances` (gap G-SS5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum ChoiceInstancesArg {
    /// As many as its data and occur max allow.
    #[default]
    Occur,
    /// Exactly one.
    One,
}

/// `xfa-layout --view-match` (gap G-RV1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum ViewMatchArg {
    /// An element naming several views needs all of them active.
    #[default]
    All,
    /// One active view is enough.
    Any,
}

/// The views `xfa-layout` processes the template for.
#[derive(Debug, Clone, Default, clap::Args)]
pub(crate) struct XfaViewFlags {
    /// Process the form for this view too, besides those the form's
    /// configuration names: elements marked for other views only, or
    /// marked to leave this one out, are dropped. Print is the view for
    /// printing. Repeatable.
    #[arg(long = "view", value_name = "NAME")]
    pub(crate) views: Vec<String>,
    /// Ignore the views the form's configuration names.
    #[arg(long)]
    pub(crate) ignore_config_views: bool,
    /// Whether an element naming several views needs all of them active or
    /// any one. The XFA spec reads both ways.
    #[arg(long, value_enum, default_value_t)]
    pub(crate) view_match: ViewMatchArg,
}

/// The data-binding settings `xfa-layout` and `xfa-fill` merge with.
#[derive(Debug, Clone, Copy, Default, clap::Args)]
pub(crate) struct XfaMergeFlags {
    /// Where a data reference with no leading $ finds its first name:
    /// the current data group's children and then each enclosing
    /// group's, as XFA 3.3 describes, or the current group's only.
    #[arg(long, value_enum, default_value_t)]
    pub(crate) scope_search: ScopeSearchArg,
    /// What a bind with a ref but no match="dataRef" does: nothing, as
    /// the XFA grammar says, or bind as a data reference.
    #[arg(long, value_enum, default_value_t)]
    pub(crate) bare_ref: BareRefArg,
    /// Whether attributes in the form's data count as data values (as
    /// XFA's default loader does) or are ignored.
    #[arg(long, value_enum, default_value_t)]
    pub(crate) data_attributes: DataAttributesArg,
    /// Where a match="global" field that found no value by name looks
    /// for the one value every copy shares: anywhere in the data, or
    /// only outside every record (the strict XFA reading).
    #[arg(long, value_enum, default_value_t)]
    pub(crate) global_scope: GlobalScopeArg,
    /// Whether match="global" on an exclusion group shares its value
    /// like a field (honour) or binds as match="once" (as-once). XFA 3.3
    /// says both in different places.
    #[arg(long, value_enum, default_value_t)]
    pub(crate) global_exclgroup: GlobalExclGroupArg,
    /// Which of several bindItems on one field fills its list: the last
    /// or the first.
    #[arg(long, value_enum, default_value_t)]
    pub(crate) bind_items_repeat: BindItemsRepeatArg,
    /// What a bindItems item with no value or label node becomes: an
    /// empty string, or nothing (the item is left out).
    #[arg(long, value_enum, default_value_t)]
    pub(crate) bind_items_missing: BindItemsMissingArg,
    /// Where a bindItems on a field bound to no data resolves: from the
    /// enclosing data scope, or not at all (the template items stay).
    #[arg(long, value_enum, default_value_t)]
    pub(crate) bind_items_unbound: BindItemsUnboundArg,
    /// Where a bindItems reference with no leading $ finds its first
    /// name; as --scope-search.
    #[arg(long, value_enum, default_value_t)]
    pub(crate) bind_items_search: ScopeSearchArg,
    /// How many copies the subform a choice subform set picks gets: as
    /// many as its data and occur max allow, or exactly one. XFA 3.3
    /// says only one is copied without saying whether that counts
    /// subforms or copies.
    #[arg(long, value_enum, default_value_t)]
    pub(crate) choice_instances: ChoiceInstancesArg,
}

/// The settings `xfa-layout` merges and lays the template with.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct XfaLayoutArgs {
    pub(crate) negative_coordinates: NegativeCoordinatesArg,
    pub(crate) nested_overflow: NestedOverflowArg,
    pub(crate) page_area_keep: PageAreaKeepArg,
    pub(crate) odd_rotate: OddRotateArg,
    pub(crate) tiled_split: TiledSplitArg,
    pub(crate) table_cell_height: TableCellHeightArg,
    pub(crate) image_dpi: u16,
    pub(crate) merge: XfaMergeFlags,
}

impl XfaMergeFlags {
    /// The core merge options these flags select.
    pub(crate) fn options(self) -> MergeOptions {
        use pdfcer_core::xfa::{
            BareRef, BindItemsMissing, BindItemsRepeat, BindItemsUnbound, ChoiceInstances,
            DataAttributes, GlobalExclGroup, GlobalScope, ScopeSearch,
        };
        MergeOptions::new()
            .with_scope_search(match self.scope_search {
                ScopeSearchArg::Upward => ScopeSearch::Upward,
                ScopeSearchArg::Children => ScopeSearch::Children,
            })
            .with_bare_ref(match self.bare_ref {
                BareRefArg::Ignore => BareRef::Ignore,
                BareRefArg::DataRef => BareRef::DataRef,
            })
            .with_data_attributes(match self.data_attributes {
                DataAttributesArg::Load => DataAttributes::Load,
                DataAttributesArg::Ignore => DataAttributes::Ignore,
            })
            .with_global_scope(match self.global_scope {
                GlobalScopeArg::Document => GlobalScope::Document,
                GlobalScopeArg::Records => GlobalScope::Records,
            })
            .with_global_exclgroup(match self.global_exclgroup {
                GlobalExclGroupArg::Honour => GlobalExclGroup::Honour,
                GlobalExclGroupArg::AsOnce => GlobalExclGroup::AsOnce,
            })
            .with_bind_items_repeat(match self.bind_items_repeat {
                BindItemsRepeatArg::Last => BindItemsRepeat::Last,
                BindItemsRepeatArg::First => BindItemsRepeat::First,
            })
            .with_bind_items_missing(match self.bind_items_missing {
                BindItemsMissingArg::Empty => BindItemsMissing::Empty,
                BindItemsMissingArg::Skip => BindItemsMissing::Skip,
            })
            .with_bind_items_unbound(match self.bind_items_unbound {
                BindItemsUnboundArg::Scope => BindItemsUnbound::Scope,
                BindItemsUnboundArg::Ignore => BindItemsUnbound::Ignore,
            })
            .with_bind_items_search(match self.bind_items_search {
                ScopeSearchArg::Upward => ScopeSearch::Upward,
                ScopeSearchArg::Children => ScopeSearch::Children,
            })
            .with_choice_instances(match self.choice_instances {
                ChoiceInstancesArg::Occur => ChoiceInstances::Occur,
                ChoiceInstancesArg::One => ChoiceInstances::One,
            })
    }
}

impl XfaLayoutArgs {
    pub(crate) fn options(self) -> LayoutOptions {
        use pdfcer_core::xfa::{
            NegativeCoordinates, NestedOverflow, OddRotate, PageAreaKeep, TableCellHeight,
            TiledSplit,
        };
        LayoutOptions::new()
            .with_negative_coordinates(match self.negative_coordinates {
                NegativeCoordinatesArg::Keep => NegativeCoordinates::Keep,
                NegativeCoordinatesArg::Clamp => NegativeCoordinates::Clamp,
            })
            .with_nested_overflow(match self.nested_overflow {
                NestedOverflowArg::Innermost => NestedOverflow::Innermost,
                NestedOverflowArg::EachAsserting => NestedOverflow::EachAsserting,
            })
            .with_page_area_keep(match self.page_area_keep {
                PageAreaKeepArg::Spec => PageAreaKeep::Spec,
                PageAreaKeepArg::Adobe => PageAreaKeep::Adobe,
            })
            .with_odd_rotate(match self.odd_rotate {
                OddRotateArg::Ignore => OddRotate::Ignore,
                OddRotateArg::Nearest => OddRotate::Nearest,
            })
            .with_tiled_split(match self.tiled_split {
                TiledSplitArg::Split => TiledSplit::Split,
                TiledSplitArg::TbOnly => TiledSplit::TbOnly,
            })
            .with_table_cell_height(match self.table_cell_height {
                TableCellHeightArg::Content => TableCellHeight::Content,
                TableCellHeightArg::Declared => TableCellHeight::Declared,
            })
            .with_image_dpi(self.image_dpi)
    }
}

/// `xfa-layout`: lay the template out and print one line per page, item and
/// warning, then a summary; with `png`, also write page images. Read-only
/// on the PDF. Exit 1 when the file has no XFA
/// template or it cannot be laid out.
pub(crate) fn cmd_xfa_layout(
    input: &Path,
    args: XfaLayoutArgs,
    views: &XfaViewFlags,
    png: Option<&XfaPngArgs>,
    font_dirs: &[PathBuf],
) -> u8 {
    let (font_env, fonts) = crate::xfa_page::xfa_font_env(font_dirs);
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let fail = |msg: &dyn std::fmt::Display| {
        eprintln!("pdfcer: {}: {msg}", input.display());
        exit::RUNTIME_ERROR
    };
    let (merged, excluded) = match read_merged(&doc, args, views) {
        Ok(m) => m,
        Err(e) => return fail(&e),
    };
    for path in &excluded {
        println!("excluded path={}", dotted(path));
    }
    let images = pdfcer_core::xfa::XfaImages::from_view(&doc.view());
    let laid =
        pdfcer_core::xfa::layout_template_with(&merged.template, &args.options(), &images, &fonts);
    let layout = match laid {
        Ok(l) => l,
        Err(e) => return fail(&e),
    };
    print_layout(&layout, &merged);
    if let Some(png) = png
        && let Err(e) = write_xfa_pngs(&doc, &merged.template, &layout, png, &font_env)
    {
        return fail(&e);
    }
    let data = if merged.bound.is_empty() {
        "with no form data merged".to_owned()
    } else {
        format!(
            "with {} value(s) merged from the form's data",
            merged.bound.len()
        )
    };
    eprintln!(
        "pdfcer: {}: these pages are pdfcer's layout of the XFA template {data}; they are not stored in the file.",
        input.display()
    );
    crate::xfa_page::disclose_fonts(input, &fonts, &[]);
    exit::SUCCESS
}

/// The document's XFA template, less the elements outside the active views
/// (returned as template paths), with its data merged in.
pub(crate) fn read_merged(
    doc: &Document,
    args: XfaLayoutArgs,
    flags: &XfaViewFlags,
) -> Result<(MergedForm, Vec<Vec<usize>>), String> {
    use pdfcer_core::xfa::{ViewMatch, Views};
    let xdp = pdfcer_core::xfa::read_xdp(&doc.view())
        .map_err(|e| e.to_string())?
        .ok_or("the file has no XFA form")?;
    let mut template = pdfcer_core::xfa::read_template(&xdp)
        .map_err(|e| e.to_string())?
        .ok_or("the XFA form has no template packet")?;
    let mut views = if flags.ignore_config_views {
        Views::new()
    } else {
        Views::from_config(&xdp).map_err(|e| e.to_string())?
    };
    for v in &flags.views {
        views = views.with_view(v.as_str());
    }
    views = views.with_matching(match flags.view_match {
        ViewMatchArg::All => ViewMatch::All,
        ViewMatchArg::Any => ViewMatch::Any,
    });
    let excluded = template.exclude_irrelevant(&views);
    let merged = pdfcer_core::xfa::merge_data(&template, &xdp, &args.merge.options())
        .map_err(|e| e.to_string())?;
    Ok((merged, excluded))
}

fn print_layout(layout: &XfaLayout, merged: &MergedForm) {
    for (i, page) in layout.pages.iter().enumerate() {
        print_page(i, page);
        for item in &page.items {
            print_item(i, item, merged.value_at(&item.path));
        }
    }
    for w in &layout.warnings {
        println!("warning {}", warning_fields(w));
    }
    print_merge_warnings(merged);
    if merged.has_data {
        println!(
            "data unfilled={} unbound-values={}",
            merged.unfilled, merged.unbound_values
        );
    }
    let items: usize = layout.pages.iter().map(|p| p.items.len()).sum();
    println!(
        "xfa-layout pages={} items={items} warnings={}",
        layout.pages.len(),
        layout.warnings.len() + merged.warnings.len()
    );
}

fn print_page(i: usize, page: &LayoutPage) {
    println!(
        "page index={i} name={:?} width={} height={} page_area={} content_areas={} items={}",
        page.name.as_deref().unwrap_or(""),
        page.width,
        page.height,
        dotted(&page.page_area),
        page.content_areas.len(),
        page.items.len()
    );
}

fn print_item(page: usize, item: &LayoutItem, value: Option<&str>) {
    let r = item.rect;
    let value = value.map_or_else(String::new, |v| format!(" value={v:?}"));
    let piece = item.fragment.map_or_else(String::new, |f| {
        format!(" fragment={}/{}", f.offset, f.height)
    });
    let turn = if item.rotate == 0 {
        String::new()
    } else {
        format!(" rotate={}", item.rotate)
    };
    println!(
        "item page={page} kind={} name={:?} path={} x={} y={} w={} h={} area={} background={} visible={}{piece}{turn}{value}",
        kind_label(item.kind),
        item.name.as_deref().unwrap_or(""),
        dotted(&item.path),
        r.x,
        r.y,
        r.w,
        r.h,
        item.content_area
            .map_or_else(|| "-".to_owned(), |a| a.to_string()),
        yes_no(item.background),
        yes_no(item.visible)
    );
}

const fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

/// A template path as `0.2.1`; `-` when empty.
pub(crate) fn dotted(path: &[usize]) -> String {
    if path.is_empty() {
        return "-".to_owned();
    }
    path.iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(".")
}

fn kind_label(k: ContainerKind) -> &'static str {
    match k {
        ContainerKind::Subform => "subform",
        ContainerKind::SubformSet => "subformSet",
        ContainerKind::Field => "field",
        ContainerKind::Draw => "draw",
        ContainerKind::ExclGroup => "exclGroup",
        ContainerKind::Area => "area",
        _ => "other",
    }
}

/// One `warning kind=data-...` line per merge warning.
pub(crate) fn print_merge_warnings(merged: &pdfcer_core::xfa::MergedForm) {
    for w in &merged.warnings {
        let (how, key) = if w.by_name {
            ("name", "name")
        } else {
            ("ref", "ref")
        };
        println!(
            "warning kind=data-{how}-{} path={} {key}={:?}",
            w.kind.as_str(),
            dotted(&w.path),
            w.reference
        );
    }
}

/// `kind=… path=…` and any detail, for one warning line.
pub(crate) fn warning_fields(w: &LayoutWarning) -> String {
    use LayoutWarning as W;
    let at = |kind: &str, path: &[usize]| format!("kind={kind} path={}", dotted(path));
    match w {
        W::InappropriateLayout { path, strategy } => {
            format!(
                "{} strategy={}",
                at("row-outside-table", path),
                format!("{strategy:?}").to_lowercase()
            )
        }
        W::QualifiedPagination => "kind=qualified-pagination path=-".to_owned(),
        W::UnboundedBoilerplate { path } => at("unbounded-boilerplate", path),
        W::InvalidBreakTarget { path, target } => {
            format!("{} target={target:?}", at("invalid-break-target", path))
        }
        W::InvalidOverflowTarget { path, target } => {
            format!("{} target={target:?}", at("invalid-overflow-target", path))
        }
        W::InvalidBookendTarget { path, target } => {
            format!("{} target={target:?}", at("invalid-bookend-target", path))
        }
        W::MinGreaterThanMax { path, attribute } => {
            format!("{} attribute={attribute}", at("min-greater-than-max", path))
        }
        W::NegativeCoordinate { path } => at("negative-coordinate", path),
        W::UnresolvedMeasurement { path, attribute } => {
            format!(
                "{} attribute={attribute}",
                at("unresolved-measurement", path)
            )
        }
        W::ContentDiscarded { path } => at("content-discarded", path),
        W::ContentOverflow { path } => at("content-overflow", path),
        W::PageAreaKeep { path } => at("page-area-keep", path),
        W::PageLimit => "kind=page-limit path=-".to_owned(),
        W::ItemLimit => "kind=item-limit path=-".to_owned(),
        _ => "kind=other path=-".to_owned(),
    }
}
