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
}

/// The settings `xfa-layout` merges and lays the template with.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct XfaLayoutArgs {
    pub(crate) negative_coordinates: NegativeCoordinatesArg,
    pub(crate) nested_overflow: NestedOverflowArg,
    pub(crate) page_area_keep: PageAreaKeepArg,
    pub(crate) merge: XfaMergeFlags,
}

impl XfaMergeFlags {
    /// The core merge options these flags select.
    pub(crate) fn options(self) -> MergeOptions {
        use pdfcer_core::xfa::{
            BareRef, DataAttributes, GlobalExclGroup, GlobalScope, ScopeSearch,
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
    }
}

impl XfaLayoutArgs {
    fn options(self) -> LayoutOptions {
        use pdfcer_core::xfa::{NegativeCoordinates, NestedOverflow, PageAreaKeep};
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
    }
}

/// `xfa-layout`: lay the template out and print one line per page, item and
/// warning, then a summary; with `png`, also write page images. Read-only
/// on the PDF. Exit 1 when the file has no XFA
/// template or it cannot be laid out.
pub(crate) fn cmd_xfa_layout(input: &Path, args: XfaLayoutArgs, png: Option<&XfaPngArgs>) -> u8 {
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
    let merged = match read_merged(&doc, args) {
        Ok(m) => m,
        Err(e) => return fail(&e),
    };
    let layout = match pdfcer_core::xfa::layout_template(&merged.template, &args.options()) {
        Ok(l) => l,
        Err(e) => return fail(&e),
    };
    print_layout(&layout, &merged);
    if let Some(png) = png
        && let Err(e) = write_xfa_pngs(&doc, &merged.template, &layout, png)
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
    exit::SUCCESS
}

/// The document's XFA template with its data merged in.
fn read_merged(doc: &Document, args: XfaLayoutArgs) -> Result<MergedForm, String> {
    let xdp = pdfcer_core::xfa::read_xdp(&doc.view())
        .map_err(|e| e.to_string())?
        .ok_or("the file has no XFA form")?;
    let template = pdfcer_core::xfa::read_template(&xdp)
        .map_err(|e| e.to_string())?
        .ok_or("the XFA form has no template packet")?;
    pdfcer_core::xfa::merge_data(&template, &xdp, &args.merge.options()).map_err(|e| e.to_string())
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
    println!(
        "item page={page} kind={} name={:?} path={} x={} y={} w={} h={} area={} background={} visible={}{value}",
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

/// `kind=… path=…` and any detail, for one warning line.
fn warning_fields(w: &LayoutWarning) -> String {
    use LayoutWarning as W;
    let at = |kind: &str, path: &[usize]| format!("kind={kind} path={}", dotted(path));
    match w {
        W::FlowedLayoutPending { path, strategy } => {
            format!(
                "{} strategy={}",
                at("flowed-layout-pending", path),
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
