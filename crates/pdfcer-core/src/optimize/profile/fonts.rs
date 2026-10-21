//! The profile keys for [`OptimizeSettings::unembed`]:
//!
//! ```toml
//! unembed-fonts = "all"          # or ["Arial", "Helvetica"]; [] unembeds none
//! unembed-font-ids = ["12 0"]    # font dictionaries by id, instead of names
//! keep-subset-tag = false        # after either key: keep the ABCDEF+ tag
//! ```

use std::fmt::Write as _;

use super::syntax::{self, Value};
use super::{boolean, object_ids, strings};
use crate::font_unembed::{SubsetTagPolicy, UnembedRequest, UnembedSelection};
use crate::optimize::OptimizeSettings;

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
        _ => return None,
    };
    Some(result)
}

/// The font keys for `settings`, one per line.
pub(super) fn write(out: &mut String, settings: &OptimizeSettings) {
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
