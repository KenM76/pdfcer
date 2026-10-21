//! `optimize --unembed-fonts`: the settings it sets and its report lines.

use std::fmt::Write as _;

use pdfcer_core::font_unembed::{UnembedPlan, UnembedRequest};
use pdfcer_core::optimize::{OptimizePlan, OptimizeSettings};

use super::json_escape;

/// Set [`OptimizeSettings::unembed`] from `--unembed-fonts` (`all`, `none`
/// or names) and `--keep-subset-tag`; no names leaves the profile's choice.
pub(crate) fn apply_args(settings: &mut OptimizeSettings, names: &[String], keep_tag: bool) {
    let request = match names {
        [] => settings.unembed.take(),
        [one] if one == "none" => None,
        [one] if one == "all" => Some(UnembedRequest::all_removable()),
        _ => Some(UnembedRequest::named(names.to_vec())),
    };
    settings.unembed = request.map(|r| if keep_tag { r.keeping_subset_tag() } else { r });
}

/// The `fonts:` block: each font unembedded and each refused, with why.
pub(crate) fn fonts_text(plan: &OptimizePlan) -> String {
    let Some(u) = &plan.unembed else {
        return String::new();
    };
    let mut out = String::new();
    let _ = writeln!(
        out,
        "fonts: {} unembedded, {} refused, {} B of font programs dropped",
        u.targets.len(),
        u.blocked.len(),
        u.bytes_reclaimable()
    );
    for t in &u.targets {
        let rename = t
            .rename
            .as_deref()
            .map_or_else(String::new, |n| format!(", renamed {n}"));
        let kept = if t.program_freed {
            String::new()
        } else {
            ", program kept: another font still uses it".to_owned()
        };
        let _ = writeln!(
            out,
            "  {:<8} unembed {} ({} B, pages {}){rename}{kept}",
            t.id.to_string(),
            t.base_font.as_deref().unwrap_or("-"),
            t.reclaimed_bytes(),
            pdfcer_core::fontinfo::format_page_ranges(&t.pages)
        );
    }
    for b in &u.blocked {
        let id =
            b.id.map_or_else(|| "direct".to_owned(), |id| id.to_string());
        let _ = writeln!(
            out,
            "  {id:<8} kept {}: {}",
            b.base_font.as_deref().unwrap_or("-"),
            b.blocker.reason()
        );
    }
    out
}

/// The JSON `fonts` member, or `null`.
pub(crate) fn fonts_json(plan: &OptimizePlan) -> String {
    plan.unembed
        .as_ref()
        .map_or_else(|| "null".to_owned(), plan_json)
}

fn plan_json(u: &UnembedPlan) -> String {
    let q = |s: &str| format!("\"{}\"", json_escape(s));
    let name = |n: Option<&str>| n.map_or_else(|| "null".to_owned(), q);
    let targets: Vec<String> = u
        .targets
        .iter()
        .map(|t| {
            let pages: Vec<String> = t.pages.iter().map(ToString::to_string).collect();
            format!(
                "{{\"id\":{},\"name\":{},\"bytes\":{},\"freed\":{},\"rename\":{},\"pages\":[{}]}}",
                q(&t.id.to_string()),
                name(t.base_font.as_deref()),
                t.reclaimed_bytes(),
                t.program_freed,
                name(t.rename.as_deref()),
                pages.join(",")
            )
        })
        .collect();
    let refused: Vec<String> = u
        .blocked
        .iter()
        .map(|b| {
            format!(
                "{{\"id\":{},\"name\":{},\"verdict\":{},\"reason\":{}}}",
                b.id.map_or_else(|| "null".to_owned(), |id| q(&id.to_string())),
                name(b.base_font.as_deref()),
                q(b.blocker.token()),
                q(b.blocker.reason())
            )
        })
        .collect();
    let unmatched: Vec<String> = u.unmatched.iter().map(|n| q(n)).collect();
    format!(
        "{{\"unembedded\":[{}],\"refused\":[{}],\"unmatched\":[{}],\"bytes\":{}}}",
        targets.join(","),
        refused.join(","),
        unmatched.join(","),
        u.bytes_reclaimable()
    )
}
