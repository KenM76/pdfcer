//! The page-numbering scripts form designers attach to a field: recognised
//! by their exact text, never executed. `xfa.layout` page/count methods
//! and the `ready` event on `$layout` are sourced in the spec RAG slice
//! `adobe_ext__xfa_layout_object_page_numbers`.

use crate::xfa::XfaNode;

/// The value `node`'s `$layout` `ready` script assigns it on page `page`
/// (1-based) of `count`, when that script is one assignment of a page
/// number or count to the node itself.
pub(super) fn value(node: &XfaNode, page: usize, count: usize) -> Option<String> {
    node.children
        .iter()
        .filter(|e| e.tag == "event" && e.attr("activity") == Some("ready"))
        .filter(|e| e.attr("ref").map(str::trim) == Some("$layout"))
        .filter_map(|e| e.child("script"))
        .find_map(|s| assigned(&s.text, page, count))
}

/// `this.rawValue = xfa.layout.page(this);` and its FormCalc (`$`)
/// spellings, with any spacing; anything else is `None`. `page` is
/// 1-based, `absPage` and `sheet` 0-based; one sheet per page is assumed
/// (simplex, gap G-LO6).
fn assigned(script: &str, page: usize, count: usize) -> Option<String> {
    let s: String = script.chars().filter(|c| !c.is_whitespace()).collect();
    let (lhs, rhs) = s.trim_end_matches(';').split_once('=')?;
    if !matches!(lhs, "this.rawValue" | "$" | "$.rawValue") {
        return None;
    }
    let call = rhs.strip_prefix("xfa.layout.")?;
    let (name, arg) = call.strip_suffix(')')?.split_once('(')?;
    let me = matches!(arg, "this" | "$");
    let n = match name {
        "page" if me => page,
        "absPage" | "sheet" if me => page.checked_sub(1)?,
        "pageCount" | "sheetCount" | "absPageCount" if arg.is_empty() => count,
        _ => return None,
    };
    Some(n.to_string())
}

#[cfg(test)]
mod tests {
    use super::assigned;

    #[test]
    fn the_designer_spellings_are_recognised_and_nothing_else() {
        assert_eq!(
            assigned("this.rawValue = xfa.layout.page(this);", 3, 9).as_deref(),
            Some("3")
        );
        assert_eq!(
            assigned("$ = xfa.layout.pageCount()", 3, 9).as_deref(),
            Some("9")
        );
        assert_eq!(
            assigned("$.rawValue = xfa.layout.absPage($)", 3, 9).as_deref(),
            Some("2")
        );
        assert_eq!(
            assigned("this.rawValue = xfa.layout.page(other);", 3, 9),
            None
        );
        assert_eq!(
            assigned("this.rawValue = xfa.layout.page(this) + 1;", 3, 9),
            None
        );
        assert_eq!(assigned("x.rawValue = xfa.layout.pageCount();", 3, 9), None);
    }
}
