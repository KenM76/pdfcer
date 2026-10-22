//! Saved optimize profiles: [`parse`] and [`to_profile`].
//!
//! A profile is a TOML file (a subset; see the grammar in `syntax.rs`):
//!
//! ```toml
//! extends = "standard"        # a preset name, or another profile
//! streams = "recompress-9"    # the default action: leave | recompress | recompress-0..9
//! images = "image(ppi=150)"   # image settings over streams; other streams keep it
//! content = "content()"       # page/form/pattern content compaction, lossless
//! allow-signed = false
//! remove-unreferenced = true  # the structure rebuild at save; all three
//! merge-duplicates = true     # false keeps the file's structure
//! object-streams = true
//! remove-invalid-links = true          # navigation clean-up; every preset
//! remove-invalid-bookmarks = true      # turns the first two on
//! remove-unreferenced-destinations = false
//! discard = ["thumbnail", "bookmarks"]  # data kinds / object classes removed, or "all"
//! smooth-tolerance = 0.1    # page points a "smooth-lines" curve may stray
//! unembed-fonts = "all"      # or names, or []; see `fonts.rs` for the font keys
//!
//! [[rule]]                    # later rules beat earlier ones
//! category = ["fonts"]        # space-audit category names
//! pages = "1-3,7"
//! min-bytes = 1000
//! max-bytes = 500000
//! filter = ["LZWDecode", "none"]
//! color-space = ["DeviceRGB"]
//! image-class = ["color", "gray"]
//! min-ppi = 150
//! max-ppi = 600
//! ids = ["12 0"]
//! action = "leave"
//!
//! [[object]]                  # per-object overrides beat every rule
//! id = "14 0"
//! action = "recompress-6"
//! ```
//!
//! Inheritance: the parent's settings are applied first. The child's
//! top-level keys then replace the parent's, its rules follow the parent's,
//! and its object overrides replace the parent's for the same id. Unknown
//! keys and tables are errors, so a misspelt key never silently does nothing.

mod fonts;
mod syntax;

use std::fmt::Write as _;

use syntax::{Kind, Table, Value};

use super::{
    ContentPatch, DiscardOptions, ImagePatch, OptimizeSettings, PageRanges, Preset, Rule, Selector,
    StreamAction,
};
use crate::object::ObjId;
use crate::space_audit::{ImageClass, SpaceCategory};

/// Largest profile accepted, in bytes.
pub const MAX_PROFILE_BYTES: usize = 1 << 20;
/// Deepest `extends` chain accepted.
pub const MAX_EXTENDS_DEPTH: usize = 8;

/// A profile that could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{profile}:{line}: {message}")]
#[non_exhaustive]
pub struct ProfileError {
    /// The profile's name, as given to [`parse`] or named by `extends`.
    pub profile: String,
    /// 1-based line; 0 for a whole-file problem.
    pub line: usize,
    /// What is wrong.
    pub message: String,
}

/// Parse profile `text` named `name`. `resolve` returns the text of a
/// profile an `extends` names that is not a [`Preset`]; core does no I/O, so
/// the shell decides what a name means (the CLI reads it as a path relative
/// to the naming profile).
///
/// # Errors
///
/// [`ProfileError`] for a syntax error, an unknown key, a bad value, a
/// resolve failure, a cycle, or a chain deeper than [`MAX_EXTENDS_DEPTH`].
///
/// # Example
///
/// ```
/// use pdfcer_core::optimize::{StreamAction, profile};
///
/// let text = "extends = \"lossless\"\n[[object]]\nid = \"7 0\"\naction = \"leave\"\n";
/// let settings = profile::parse("mine.toml", text, &mut |n| Err(format!("no {n}")))?;
/// assert_eq!(settings.streams, StreamAction::Recompress { level: 9 });
/// assert_eq!(settings.objects.len(), 1);
/// # Ok::<(), profile::ProfileError>(())
/// ```
pub fn parse(
    name: &str,
    text: &str,
    resolve: &mut dyn FnMut(&str) -> Result<String, String>,
) -> Result<OptimizeSettings, ProfileError> {
    parse_chain(&mut vec![name.to_owned()], text, resolve)
}

fn parse_chain(
    chain: &mut Vec<String>,
    text: &str,
    resolve: &mut dyn FnMut(&str) -> Result<String, String>,
) -> Result<OptimizeSettings, ProfileError> {
    let name = chain.last().cloned().unwrap_or_default();
    let err = |line, message: String| ProfileError {
        profile: name.clone(),
        line,
        message,
    };
    if text.len() > MAX_PROFILE_BYTES {
        return Err(err(0, format!("larger than {MAX_PROFILE_BYTES} bytes")));
    }
    let tables = syntax::tables(text).map_err(|(l, m)| err(l, m))?;
    let top = tables.first().map_or(&[][..], |t| t.entries.as_slice());
    let mut settings = match top.iter().find(|(k, _, _)| k == "extends") {
        None => OptimizeSettings::new(),
        Some((_, value, line)) => {
            let parent = string(value).map_err(|m| err(*line, m))?;
            inherit(chain, parent, resolve).map_err(|m| err(*line, m))?
        }
    };
    for table in &tables {
        apply_table(&mut settings, table).map_err(|(l, m)| err(l, m))?;
    }
    Ok(settings)
}

/// The on/off setting a top-level key names.
fn switch<'a>(settings: &'a mut OptimizeSettings, key: &str) -> Option<&'a mut bool> {
    let st = &mut settings.structure;
    let c = &mut settings.cleanup;
    Some(match key {
        "remove-unreferenced" => &mut st.remove_unreferenced,
        "merge-duplicates" => &mut st.merge_duplicates,
        "object-streams" => &mut st.object_streams,
        "linearize" => &mut st.linearize,
        "remove-invalid-links" => &mut c.invalid_links,
        "remove-invalid-bookmarks" => &mut c.invalid_bookmarks,
        "remove-unreferenced-destinations" => &mut c.unreferenced_destinations,
        _ => return None,
    })
}

fn inherit(
    chain: &mut Vec<String>,
    parent: &str,
    resolve: &mut dyn FnMut(&str) -> Result<String, String>,
) -> Result<OptimizeSettings, String> {
    if let Some(preset) = Preset::from_name(parent) {
        return Ok(preset.settings());
    }
    if chain.iter().any(|c| c == parent) {
        return Err(format!("extends cycle through {parent:?}"));
    }
    if chain.len() >= MAX_EXTENDS_DEPTH {
        return Err(format!("extends chain deeper than {MAX_EXTENDS_DEPTH}"));
    }
    let text = resolve(parent)?;
    chain.push(parent.to_owned());
    let out = parse_chain(chain, &text, resolve).map_err(|e| e.to_string());
    chain.pop();
    out
}

fn apply_table(settings: &mut OptimizeSettings, table: &Table) -> Result<(), (usize, String)> {
    match table.kind {
        Kind::Top => {
            for (key, value, line) in &table.entries {
                let at = |m| (*line, m);
                match key.as_str() {
                    "extends" => {}
                    "streams" => settings.streams = action(value).map_err(at)?,
                    "images" => settings.images = images(value).map_err(at)?,
                    "content" => {
                        settings.content =
                            Some(ContentPatch::parse(string(value).map_err(at)?).map_err(at)?);
                    }
                    "allow-signed" => settings.allow_signed = boolean(value).map_err(at)?,
                    "discard" => settings.discard = discard(value).map_err(at)?,
                    "smooth-tolerance" => {
                        settings.smooth_tolerance = tolerance(value).map_err(at)?
                    }
                    k if let Some(r) = fonts::apply(settings, k, value) => r.map_err(at)?,
                    k => {
                        let flag = switch(settings, k)
                            .ok_or_else(|| at(format!("unknown key {key:?}")))?;
                        *flag = boolean(value).map_err(at)?;
                    }
                }
            }
        }
        Kind::Rule => settings.rules.push(rule(table)?),
        Kind::Object => {
            let id = required(table, "id", |v| {
                object_ids(v)?
                    .first()
                    .copied()
                    .ok_or_else(|| "empty id".to_owned())
            })?;
            let act = required(table, "action", action)?;
            if let Some((key, _, line)) = table
                .entries
                .iter()
                .find(|(k, _, _)| k != "id" && k != "action")
            {
                return Err((*line, format!("unknown key {key:?} in [[object]]")));
            }
            settings.objects.insert(id, act);
        }
    }
    Ok(())
}

fn required<T>(
    table: &Table,
    key: &str,
    read: impl Fn(&Value) -> Result<T, String>,
) -> Result<T, (usize, String)> {
    let (_, value, line) = table
        .entries
        .iter()
        .find(|(k, _, _)| k == key)
        .ok_or((table.line, format!("missing {key}")))?;
    read(value).map_err(|m| (*line, m))
}

fn rule(table: &Table) -> Result<Rule, (usize, String)> {
    let action = required(table, "action", action)?;
    let mut s = Selector::new();
    for (key, value, line) in &table.entries {
        let at = |m| (*line, m);
        match key.as_str() {
            "action" => {}
            "category" => {
                s.categories = strings(value)
                    .map_err(at)?
                    .iter()
                    .map(|c| category(c))
                    .collect::<Result<_, _>>()
                    .map_err(at)?
            }
            "pages" => s.pages = Some(PageRanges::parse(string(value).map_err(at)?).map_err(at)?),
            "min-bytes" => s.min_bytes = Some(unsigned(value).map_err(at)?),
            "max-bytes" => s.max_bytes = Some(unsigned(value).map_err(at)?),
            "filter" => s.filters = strings(value).map_err(at)?,
            "color-space" => s.color_spaces = strings(value).map_err(at)?,
            "image-class" => s.image_classes = image_classes(value).map_err(at)?,
            "min-ppi" => s.min_ppi = Some(number(value).map_err(at)?),
            "max-ppi" => s.max_ppi = Some(number(value).map_err(at)?),
            "ids" => s.ids = object_ids(value).map_err(at)?,
            _ => return Err(at(format!("unknown key {key:?} in [[rule]]"))),
        }
    }
    Ok(Rule { select: s, action })
}

fn image_classes(v: &Value) -> Result<Vec<ImageClass>, String> {
    strings(v)?
        .iter()
        .map(|c| {
            ImageClass::from_name(c).ok_or_else(|| {
                format!("unknown image class {c:?}; expected color, gray, mono or indexed")
            })
        })
        .collect()
}

fn string(v: &Value) -> Result<&str, String> {
    match v {
        Value::Str(s) => Ok(s),
        _ => Err("expected a string".to_owned()),
    }
}

fn strings(v: &Value) -> Result<Vec<String>, String> {
    match v {
        Value::Array(items) => items.iter().map(|i| string(i).map(str::to_owned)).collect(),
        _ => Ok(vec![string(v)?.to_owned()]),
    }
}

fn boolean(v: &Value) -> Result<bool, String> {
    match v {
        Value::Bool(b) => Ok(*b),
        _ => Err("expected true or false".to_owned()),
    }
}

fn unsigned(v: &Value) -> Result<u64, String> {
    match v {
        Value::Int(n) => {
            u64::try_from(*n).map_err(|_| "expected a non-negative integer".to_owned())
        }
        _ => Err("expected an integer".to_owned()),
    }
}

fn number(v: &Value) -> Result<f64, String> {
    match v {
        #[allow(clippy::cast_precision_loss)] // a ppi bound; precision past 2^53 is meaningless
        Value::Int(n) => Ok(*n as f64),
        Value::Float(f) => Ok(*f),
        _ => Err("expected a number".to_owned()),
    }
}

fn tolerance(v: &Value) -> Result<f64, String> {
    let t = number(v)?;
    if t.is_finite() && t > 0.0 {
        Ok(t)
    } else {
        Err(format!(
            "smooth-tolerance must be a positive number of points, not {t}"
        ))
    }
}

/// A list of [`DiscardOptions::known_names`].
fn discard(v: &Value) -> Result<DiscardOptions, String> {
    let mut out = DiscardOptions::none();
    for name in strings(v)? {
        out = out
            .with_named(&name)
            .ok_or_else(|| format!("unknown discard kind {name:?}"))?;
    }
    Ok(out)
}

fn action(v: &Value) -> Result<StreamAction, String> {
    StreamAction::parse(string(v)?)
}

fn images(v: &Value) -> Result<ImagePatch, String> {
    ImagePatch::parse(string(v)?)
}

fn category(name: &str) -> Result<SpaceCategory, String> {
    SpaceCategory::ALL
        .into_iter()
        .find(|c| c.as_str() == name)
        .ok_or_else(|| format!("unknown category {name:?}"))
}

/// `"12 0"`, `"12"` or `12`, alone or in an array.
fn object_ids(v: &Value) -> Result<Vec<ObjId>, String> {
    let one = |v: &Value| -> Result<ObjId, String> {
        let bad = || "expected an object id such as \"12 0\"".to_owned();
        let (num, generation) = match v {
            Value::Int(n) => (u32::try_from(*n).map_err(|_| bad())?, 0),
            Value::Str(s) => {
                let mut parts = s.split_whitespace();
                let num = parts.next().and_then(|p| p.parse().ok()).ok_or_else(bad)?;
                let generation = parts
                    .next()
                    .map_or(Some(0), |p| p.parse().ok())
                    .ok_or_else(bad)?;
                if parts.next().is_some() {
                    return Err(bad());
                }
                (num, generation)
            }
            _ => return Err(bad()),
        };
        if num == 0 {
            return Err(bad());
        }
        Ok(ObjId::new(num, generation))
    };
    match v {
        Value::Array(items) => items.iter().map(one).collect(),
        _ => Ok(vec![one(v)?]),
    }
}

/// One rule from a one-line spec: whitespace-separated `key=value` pairs
/// using the `[[rule]]` keys, list values comma-separated, for example
/// `category=images,fonts pages=2- min-bytes=4096 action=leave`.
///
/// # Errors
///
/// [`ProfileError`] (line 0) for a malformed pair, unknown key or bad value.
///
/// # Example
///
/// ```
/// use pdfcer_core::optimize::{StreamAction, profile};
///
/// let rule = profile::parse_rule("category=fonts min-bytes=4096 action=leave")?;
/// assert_eq!(rule.action, StreamAction::Leave);
/// assert_eq!(rule.select.min_bytes, Some(4096));
/// # Ok::<(), profile::ProfileError>(())
/// ```
pub fn parse_rule(spec: &str) -> Result<Rule, ProfileError> {
    let err = |message: String| ProfileError {
        profile: format!("rule {spec:?}"),
        line: 0,
        message,
    };
    let mut table = Table {
        kind: Kind::Rule,
        line: 0,
        entries: Vec::new(),
    };
    for pair in spec.split_whitespace() {
        let (key, raw) = pair
            .split_once('=')
            .ok_or_else(|| err(format!("expected key=value, found {pair:?}")))?;
        let value = match key {
            "min-bytes" | "max-bytes" => raw
                .parse()
                .map_or_else(|_| Value::Str(raw.to_owned()), Value::Int),
            "min-ppi" | "max-ppi" => raw
                .parse()
                .map_or_else(|_| Value::Str(raw.to_owned()), Value::Float),
            "category" | "filter" | "color-space" | "image-class" | "ids" => {
                Value::Array(raw.split(',').map(|v| Value::Str(v.to_owned())).collect())
            }
            _ => Value::Str(raw.to_owned()),
        };
        if table.entries.iter().any(|(k, _, _)| k == key) {
            return Err(err(format!("duplicate key {key:?}")));
        }
        table.entries.push((key.to_owned(), value, 0));
    }
    rule(&table).map_err(|(_, m)| err(m))
}

/// One per-object override from `ID=ACTION`, where `ID` is `12` or `12 0`.
///
/// # Errors
///
/// [`ProfileError`] (line 0) for a malformed id or action.
pub fn parse_object(spec: &str) -> Result<(ObjId, StreamAction), ProfileError> {
    let err = |message: String| ProfileError {
        profile: format!("object {spec:?}"),
        line: 0,
        message,
    };
    let (id, act) = spec
        .split_once('=')
        .ok_or_else(|| err("expected ID=ACTION".to_owned()))?;
    let id = object_ids(&Value::Str(id.trim().to_owned()))
        .map_err(err)?
        .first()
        .copied()
        .ok_or_else(|| err("empty id".to_owned()))?;
    let act = StreamAction::parse(act.trim()).map_err(err)?;
    Ok((id, act))
}

/// `settings` as a complete profile with no `extends`. [`parse`] reads it
/// back to equal settings.
#[must_use]
pub fn to_profile(settings: &OptimizeSettings) -> String {
    let mut out = String::from("# pdfcer optimize profile\n");
    let _ = writeln!(
        out,
        "streams = {}",
        syntax::quote(&settings.streams.to_profile_string())
    );
    if settings.images != ImagePatch::new() {
        let _ = writeln!(
            out,
            "images = {}",
            syntax::quote(&settings.images.to_string())
        );
    }
    if let Some(patch) = settings.content {
        let _ = writeln!(out, "content = {}", syntax::quote(&patch.to_string()));
    }
    let _ = writeln!(out, "allow-signed = {}", settings.allow_signed);
    let st = &settings.structure;
    let _ = writeln!(out, "remove-unreferenced = {}", st.remove_unreferenced);
    let _ = writeln!(out, "merge-duplicates = {}", st.merge_duplicates);
    let _ = writeln!(out, "object-streams = {}", st.object_streams);
    let _ = writeln!(out, "linearize = {}", st.linearize);
    let c = &settings.cleanup;
    let _ = writeln!(out, "remove-invalid-links = {}", c.invalid_links);
    let _ = writeln!(out, "remove-invalid-bookmarks = {}", c.invalid_bookmarks);
    let _ = writeln!(
        out,
        "remove-unreferenced-destinations = {}",
        c.unreferenced_destinations
    );
    let kinds: Vec<String> = settings.discard.names().map(syntax::quote).collect();
    let _ = writeln!(out, "discard = [{}]", kinds.join(", "));
    let _ = writeln!(out, "smooth-tolerance = {:?}", settings.smooth_tolerance);
    fonts::write(&mut out, settings);
    for rule in &settings.rules {
        out.push_str("\n[[rule]]\n");
        write_selector(&mut out, &rule.select);
        let _ = writeln!(
            out,
            "action = {}",
            syntax::quote(&rule.action.to_profile_string())
        );
    }
    for (id, act) in &settings.objects {
        out.push_str("\n[[object]]\n");
        let _ = writeln!(out, "id = {}", syntax::quote(&id.to_string()));
        let _ = writeln!(out, "action = {}", syntax::quote(&act.to_profile_string()));
    }
    out
}

fn write_selector(out: &mut String, s: &Selector) {
    let list = |items: Vec<String>| {
        let quoted: Vec<String> = items.iter().map(|i| syntax::quote(i)).collect();
        format!("[{}]", quoted.join(", "))
    };
    if !s.categories.is_empty() {
        let names = s.categories.iter().map(|c| c.as_str().to_owned()).collect();
        let _ = writeln!(out, "category = {}", list(names));
    }
    if let Some(p) = &s.pages {
        let _ = writeln!(out, "pages = {}", syntax::quote(&p.to_string()));
    }
    if let Some(n) = s.min_bytes {
        let _ = writeln!(out, "min-bytes = {n}");
    }
    if let Some(n) = s.max_bytes {
        let _ = writeln!(out, "max-bytes = {n}");
    }
    if !s.filters.is_empty() {
        let _ = writeln!(out, "filter = {}", list(s.filters.clone()));
    }
    if !s.color_spaces.is_empty() {
        let _ = writeln!(out, "color-space = {}", list(s.color_spaces.clone()));
    }
    if !s.image_classes.is_empty() {
        let names = s
            .image_classes
            .iter()
            .map(|c| c.as_str().to_owned())
            .collect();
        let _ = writeln!(out, "image-class = {}", list(names));
    }
    if let Some(n) = s.min_ppi {
        let _ = writeln!(out, "min-ppi = {n:?}");
    }
    if let Some(n) = s.max_ppi {
        let _ = writeln!(out, "max-ppi = {n:?}");
    }
    if !s.ids.is_empty() {
        let _ = writeln!(
            out,
            "ids = {}",
            list(s.ids.iter().map(ToString::to_string).collect())
        );
    }
}

#[cfg(test)]
mod tests;
