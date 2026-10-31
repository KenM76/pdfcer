//! The profile keys for [`OptimizeSettings::unembed`] and
//! [`OptimizeSettings::resubset`]:
//!
//! ```toml
//! unembed-fonts = "all"          # or ["Arial", "Helvetica"]; [] unembeds none
//! unembed-font-ids = ["12 0"]    # font dictionaries by id, instead of names
//! keep-subset-tag = false        # after either key: keep the ABCDEF+ tag
//! resubset-fonts = true          # cut programs to the glyphs drawn
//! merge-font-subsets = true      # after it: merge copies of one face
//! ```
//!
//! `resubset-fonts = true` parses to [`ResubsetFonts::unbound`]: the shell
//! installs its cutter with [`OptimizeSettings::install_resubsetter`].

use std::fmt::Write as _;

use super::syntax::{self, Value};
use super::{boolean, object_ids, strings};
use crate::font_unembed::{SubsetTagPolicy, UnembedRequest, UnembedSelection};
use crate::optimize::OptimizeSettings;
use crate::optimize::resubset::ResubsetFonts;

/// Apply `key` if it is a font key: `None` when it is not one.
pub(super) fn apply(
    settings: &mut OptimizeSettings,
    key: &str,
    value: &Value,
) -> Option<Result<(), String>> {
    let result = match key {
        "unembed-fonts" => strings(value).map(|names| {
            settings.unembed = match names.as_slice() {
                [] => None,
                [all] if all == "all" => Some(UnembedRequest::all_removable()),
                _ => Some(UnembedRequest::named(names)),
            };
        }),
        "unembed-font-ids" => object_ids(value).map(|ids| {
            settings.unembed = (!ids.is_empty()).then(|| UnembedRequest::objects(ids));
        }),
        "keep-subset-tag" => boolean(value).and_then(|keep| match settings.unembed.as_mut() {
            Some(r) => {
                r.subset_tag = if keep {
                    SubsetTagPolicy::Keep
                } else {
                    SubsetTagPolicy::Strip
                };
                Ok(())
            }
            None => {
                Err("keep-subset-tag needs unembed-fonts or unembed-font-ids before it".to_owned())
            }
        }),
        "resubset-fonts" => boolean(value).map(|on| {
            settings.resubset =
                on.then(|| settings.resubset.unwrap_or_else(ResubsetFonts::unbound));
        }),
        "merge-font-subsets" => boolean(value).and_then(|merge| match settings.resubset.as_mut() {
            Some(r) => {
                r.merge = merge;
                Ok(())
            }
            None => Err("merge-font-subsets needs resubset-fonts = true before it".to_owned()),
        }),
        _ => return None,
    };
    Some(result)
}

/// The font keys for `settings`, one per line.
pub(super) fn write(out: &mut String, settings: &OptimizeSettings) {
    write_unembed(out, settings);
    match &settings.resubset {
        Some(r) => {
            let _ = writeln!(
                out,
                "resubset-fonts = true
merge-font-subsets = {}",
                r.merge
            );
        }
        None => out.push_str(
            "resubset-fonts = false
",
        ),
    }
}

fn write_unembed(out: &mut String, settings: &OptimizeSettings) {
    let Some(request) = &settings.unembed else {
        out.push_str("unembed-fonts = []\n");
        return;
    };
    let list = |items: Vec<String>| {
        let quoted: Vec<String> = items.iter().map(|i| syntax::quote(i)).collect();
        format!("[{}]", quoted.join(", "))
    };
    match &request.selection {
        UnembedSelection::Named(names) => {
            let _ = writeln!(out, "unembed-fonts = {}", list(names.clone()));
        }
        UnembedSelection::Objects(ids) => {
            let ids = ids.iter().map(ToString::to_string).collect();
            let _ = writeln!(out, "unembed-font-ids = {}", list(ids));
        }
        _ => out.push_str("unembed-fonts = \"all\"\n"),
    }
    let _ = writeln!(
        out,
        "keep-subset-tag = {}",
        request.subset_tag == SubsetTagPolicy::Keep
    );
}
