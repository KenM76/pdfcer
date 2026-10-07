//! `pdfcer 3d-tree`: a PRC model's assembly tree.

use super::*;

/// `pdfcer 3d-tree`: reads 3D artwork `index` and prints its tree; exit 9 when refused.
pub(crate) fn cmd_tree_3d(input: &Path, index: usize, json: bool) -> u8 {
    let data = match artwork_bytes(input, index) {
        Ok((data, _)) => data,
        Err(code) => return code,
    };
    tree_from_bytes(input, index, &data, json)
}

#[cfg(not(feature = "3d"))]
fn tree_from_bytes(input: &Path, index: usize, _data: &[u8], _json: bool) -> u8 {
    no_3d_feature(input, index, "3d-tree")
}

#[cfg(feature = "3d")]
fn tree_from_bytes(input: &Path, index: usize, data: &[u8], json: bool) -> u8 {
    let nodes = if data.starts_with(b"PRC") {
        pdfcer_3d::PrcFile::parse(data).and_then(|f| f.model_tree())
    } else {
        eprintln!(
            "pdfcer: {}: 3D artwork {index}: not a PRC model; only PRC is decoded (use \
             `3d-extract` for the bytes)",
            input.display()
        );
        return exit::EDIT_REFUSED;
    };
    let nodes = match nodes {
        Ok(nodes) => nodes,
        Err(err) => {
            eprintln!("pdfcer: {}: 3D artwork {index}: {err}", input.display());
            return exit::EDIT_REFUSED;
        }
    };
    if json {
        println!("{}", tree_json(&nodes));
    } else {
        print_tree(&nodes);
    }
    exit::SUCCESS
}

/// What `name_from` says, as the CLI spells it.
#[cfg(feature = "3d")]
fn name_from(n: &pdfcer_3d::ModelNode) -> &'static str {
    match n.name_from {
        pdfcer_3d::NameSource::Occurrence => "occurrence",
        pdfcer_3d::NameSource::Prototype => "prototype",
        pdfcer_3d::NameSource::Part => "part",
        _ => "none",
    }
}

#[cfg(feature = "3d")]
fn print_tree(nodes: &[pdfcer_3d::ModelNode]) {
    let mut borrowed = 0;
    for n in nodes {
        let mut tags = Vec::new();
        if n.hidden {
            tags.push("hidden".to_owned());
        }
        if n.suppressed {
            tags.push("suppressed".to_owned());
        }
        if !n.drawn && !n.hidden && !n.suppressed {
            tags.push("not drawn".to_owned());
        }
        if matches!(name_from(n), "prototype" | "part") {
            borrowed += 1;
            tags.push(format!("name from {}", name_from(n)));
        }
        let parts = n.placements.len();
        if n.drawn && parts > 0 {
            tags.push(format!("{parts} placement(s)"));
        }
        let tags = if tags.is_empty() {
            String::new()
        } else {
            format!("  [{}]", tags.join(", "))
        };
        println!("{}{}{tags}", "  ".repeat(n.depth), n.label());
    }
    let drawn = nodes.iter().filter(|n| n.drawn).count();
    println!(
        "nodes={} drawn={drawn} not_drawn={}",
        nodes.len(),
        nodes.len() - drawn
    );
    if borrowed > 0 {
        println!(
            "note: {borrowed} occurrence(s) have no name of their own; pdfcer shows their \
             prototype's or part's"
        );
    }
}

#[cfg(feature = "3d")]
fn tree_json(nodes: &[pdfcer_3d::ModelNode]) -> String {
    let rows: Vec<String> = nodes
        .iter()
        .map(|n| {
            let name = match &n.name {
                Some(s) => format!("\"{}\"", crate::extract::json_escape(s)),
                None => "null".to_owned(),
            };
            let parent = n.parent.map_or("null".to_owned(), |p| p.to_string());
            format!(
                "{{\"name\":{name},\"name_from\":\"{}\",\"parent\":{parent},\"depth\":{},\
                 \"file_structure\":{},\"occurrence\":{},\"hidden\":{},\"suppressed\":{},\
                 \"drawn\":{},\"has_part\":{},\"placements\":[{},{}]}}",
                name_from(n),
                n.depth,
                n.file_structure,
                n.occurrence,
                n.hidden,
                n.suppressed,
                n.drawn,
                n.has_part,
                n.placements.start,
                n.placements.end
            )
        })
        .collect();
    format!("{{\"nodes\":[{}]}}", rows.join(","))
}
