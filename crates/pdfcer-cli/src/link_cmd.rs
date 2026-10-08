//! `add-link`, `set-link-target`, `set-link-border` — author and edit
//! `/Link` annotations (ISO 32000-1 §12.5.6.5).

use super::*;
use pdfcer_core::edit::{LinkBorder, LinkTarget, StyleEdit};
use pdfcer_core::outline::DestView;

/// Where a link goes: exactly one of `--to-page`, `--dest-name`, `--uri`.
#[derive(Debug, Clone, Default, clap::Args)]
pub(crate) struct LinkTargetFlags {
    /// Go to this page, 1-BASED. The whole page is fitted unless `--top`
    /// is given.
    #[arg(long, value_name = "PAGE")]
    pub(crate) to_page: Option<usize>,
    /// With `--to-page`: scroll so this user-space Y is at the top of the
    /// window (`/XYZ null top null`) instead of fitting the page.
    #[arg(long, allow_hyphen_values = true)]
    pub(crate) top: Option<f64>,
    /// Go to a NAMED destination. It must already exist (`add-named-dest`
    /// defines one); an undefined name is refused.
    #[arg(long, value_name = "NAME")]
    pub(crate) dest_name: Option<String>,
    /// Open this URI (`/A << /S /URI >>`). It must be 7-bit ASCII:
    /// percent-encode anything else first (§12.6.4.7).
    #[arg(long, value_name = "URL")]
    pub(crate) uri: Option<String>,
}

/// A visible link border. Without `--border-width` the link has none.
#[derive(Debug, Clone, Default, clap::Args)]
pub(crate) struct LinkBorderFlags {
    /// Border width in points, greater than zero.
    #[arg(long, value_name = "POINTS")]
    pub(crate) border_width: Option<f64>,
    /// Border colour as `RRGGBB` hex. Default `000000`.
    #[arg(long, value_name = "RRGGBB")]
    pub(crate) border_color: Option<String>,
    /// Dash pattern as comma-separated point lengths, e.g. `3,2`. Omit
    /// for a solid border.
    #[arg(long)]
    pub(crate) dash: Option<String>,
}

impl LinkTargetFlags {
    fn to_target(&self) -> Result<LinkTarget, String> {
        let given = [
            self.to_page.is_some(),
            self.dest_name.is_some(),
            self.uri.is_some(),
        ];
        if given.iter().filter(|g| **g).count() != 1 {
            return Err("give exactly one of --to-page, --dest-name, --uri".to_owned());
        }
        if self.top.is_some() && self.to_page.is_none() {
            return Err("--top positions a view within a page; it needs --to-page".to_owned());
        }
        if let Some(page) = self.to_page {
            let page_index = page
                .checked_sub(1)
                .ok_or_else(|| "--to-page is 1-based; 0 is not a page".to_owned())?;
            let view = match self.top {
                Some(top) => DestView::Xyz {
                    left: None,
                    top: Some(top),
                    zoom: None,
                },
                None => DestView::Fit,
            };
            return Ok(LinkTarget::Page { page_index, view });
        }
        if let Some(name) = &self.dest_name {
            return Ok(LinkTarget::Named(name.as_bytes().to_vec()));
        }
        Ok(LinkTarget::Uri(self.uri.clone().unwrap_or_default()))
    }
}

impl LinkBorderFlags {
    /// `None` when no `--border-width` was given.
    fn to_border(&self) -> Result<Option<LinkBorder>, String> {
        let Some(width) = self.border_width else {
            if self.border_color.is_some() || self.dash.is_some() {
                return Err("--border-color and --dash need --border-width".to_owned());
            }
            return Ok(None);
        };
        let color = match &self.border_color {
            Some(hex) => parse_color(hex)?,
            None => pdfcer_core::annot_author::Color::Gray(0.0),
        };
        let border = LinkBorder::new(width, color);
        Ok(Some(match parse_dash_edit(self.dash.as_deref())? {
            Some(StyleEdit::Set(dash)) => border.with_dash(dash),
            _ => border,
        }))
    }
}

/// Print `msg` and return the refusal code.
fn refuse(msg: &str) -> u8 {
    eprintln!("pdfcer: {msg}");
    exit::EDIT_REFUSED
}

/// Implement `pdfcer add-link`.
pub(crate) fn cmd_add_link(
    input: &Path,
    (page, rect): (usize, &str),
    target: &LinkTargetFlags,
    border: &LinkBorderFlags,
    output: &Path,
    mode: SaveMode,
) -> u8 {
    let Some(page_index) = page.checked_sub(1) else {
        return refuse("--page is 1-based; 0 is not a page");
    };
    let parsed = rect_from(rect).and_then(|r| Ok((r, target.to_target()?, border.to_border()?)));
    let (rect, target, border) = match parsed {
        Ok(p) => p,
        Err(msg) => return refuse(&msg),
    };
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let id = match session.add_link(page_index, rect, &target, border.as_ref()) {
        Ok(id) => id,
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
        Ok(saved) => saved,
        Err(code) => return code,
    };
    println!(
        "add-link {} page {page} -> {}\n  obj={} border={}",
        input.display(),
        output.display(),
        id.num,
        u32::from(border.is_some()),
    );
    finish_edit(input, &saved)
}

/// Implement `pdfcer set-link-target`.
pub(crate) fn cmd_set_link_target(
    input: &Path,
    (page, index): (usize, usize),
    target: &LinkTargetFlags,
    output: &Path,
    mode: SaveMode,
) -> u8 {
    let target = match target.to_target() {
        Ok(t) => t,
        Err(msg) => return refuse(&msg),
    };
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let annot_id = match resolve_annotation(&session, input, page, index) {
        Ok(id) => id,
        Err(code) => return code,
    };
    let change = match session.set_link_target(annot_id, &target) {
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
        Ok(saved) => saved,
        Err(code) => return code,
    };
    println!(
        "set-link-target {} page {page} index {index} -> {}",
        input.display(),
        output.display()
    );
    if let Some(action) = &change.replaced_action {
        // Disclosed: a link's old action (possibly a script) is gone.
        println!("  replaced a /{action} action");
    }
    println!(
        "  obj={} replaced_action={}",
        change.annot_id.num,
        change.replaced_action.as_deref().unwrap_or("none")
    );
    finish_edit(input, &saved)
}

/// Implement `pdfcer set-link-border`.
pub(crate) fn cmd_set_link_border(
    input: &Path,
    (page, index): (usize, usize),
    none: bool,
    border: &LinkBorderFlags,
    output: &Path,
    mode: SaveMode,
) -> u8 {
    let border = match (none, border.to_border()) {
        (true, Ok(None)) => None,
        (false, Ok(Some(b))) => Some(b),
        (true, Ok(Some(_))) => return refuse("--none conflicts with --border-width"),
        (false, Ok(None)) => return refuse("set-link-border needs --border-width or --none"),
        (_, Err(msg)) => return refuse(&msg),
    };
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let annot_id = match resolve_annotation(&session, input, page, index) {
        Ok(id) => id,
        Err(code) => return code,
    };
    let change = match session.set_link_border(annot_id, border.as_ref()) {
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
        Ok(saved) => saved,
        Err(code) => return code,
    };
    println!(
        "set-link-border {} page {page} index {index} -> {}\n  obj={} border={} appearance_replaced={}",
        input.display(),
        output.display(),
        change.annot_id.num,
        u32::from(border.is_some()),
        u32::from(change.appearance_replaced),
    );
    finish_edit(input, &saved)
}
