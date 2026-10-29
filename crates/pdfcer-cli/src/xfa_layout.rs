//! `xfa-layout`: print the pages pdfcer computes from an XFA template.

use super::*;

use pdfcer_core::xfa::{
    ContainerKind, LayoutItem, LayoutOptions, LayoutPage, LayoutWarning, XfaLayout,
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

/// The settings `xfa-layout` lays the template with.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct XfaLayoutArgs {
    pub(crate) negative_coordinates: NegativeCoordinatesArg,
    pub(crate) nested_overflow: NestedOverflowArg,
    pub(crate) page_area_keep: PageAreaKeepArg,
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
    let template = match pdfcer_core::xfa::read_xdp(&doc.view()) {
        Err(e) => return fail(&e),
        Ok(None) => return fail(&"the file has no XFA form"),
        Ok(Some(xdp)) => match pdfcer_core::xfa::read_template(&xdp) {
            Err(e) => return fail(&e),
            Ok(None) => return fail(&"the XFA form has no template packet"),
            Ok(Some(t)) => t,
        },
    };
    let layout = match pdfcer_core::xfa::layout_template(&template, &args.options()) {
        Ok(l) => l,
        Err(e) => return fail(&e),
    };
    print_layout(&layout);
    if let Some(png) = png
        && let Err(e) = write_xfa_pngs(&doc, &template, &layout, png)
    {
        return fail(&e);
    }
    eprintln!(
        "pdfcer: {}: these pages are pdfcer's layout of the XFA template with no form data merged; they are not stored in the file.",
        input.display()
    );
    exit::SUCCESS
}

fn print_layout(layout: &XfaLayout) {
    for (i, page) in layout.pages.iter().enumerate() {
        print_page(i, page);
        for item in &page.items {
            print_item(i, item);
        }
    }
    for w in &layout.warnings {
        println!("warning {}", warning_fields(w));
    }
    let items: usize = layout.pages.iter().map(|p| p.items.len()).sum();
    println!(
        "xfa-layout pages={} items={items} warnings={}",
        layout.pages.len(),
        layout.warnings.len()
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

fn print_item(page: usize, item: &LayoutItem) {
    let r = item.rect;
    println!(
        "item page={page} kind={} name={:?} path={} x={} y={} w={} h={} area={} background={} visible={}",
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
