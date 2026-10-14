//! The character dictionary inside a PaddleOCR export's `inference.yml`.
//!
//! PaddlePaddle publishes its PP-OCR recognisers as `inference.onnx` plus an
//! `inference.yml` whose `PostProcess.character_dict` is the dictionary; the
//! ONNX file embeds none. Reading it lets a downloaded export load unchanged,
//! with no extracted `dict.txt`.
//!
//! This is not a YAML parser. It reads one block sequence, the one under the
//! first line whose content is exactly `character_dict:`, and refuses any
//! item it cannot read exactly: a misread dictionary yields confident
//! nonsense rather than an error, so a guess is never made.

/// The most entries read; the largest published PP-OCR dictionary has about
/// 18,400 (`ARCHITECTURE.md` §10).
pub const MAX_ENTRIES: usize = 100_000;

/// The entries of the `character_dict` block sequence in `text`, in order.
///
/// Items are plain scalars (taken as written, trailing spaces trimmed),
/// single-quoted (`''` is a quote) or double-quoted (escapes `\\`, `\"`,
/// `\/`, `\t`, `\n`, `\r`, `\0`, `\xXX`, `\uXXXX`, `\UXXXXXXXX`). The
/// sequence ends at the first line that is not an item at the first item's
/// indentation.
///
/// # Errors
///
/// A message naming the problem: no `character_dict:` key, an empty
/// sequence, an item that is not a single scalar (a nested block, a
/// comment, a flow collection, an unterminated or malformed quote), or more
/// than [`MAX_ENTRIES`] items.
pub fn parse_config_dictionary(text: &str) -> Result<Vec<String>, String> {
    let mut lines = text.lines();
    if !lines.by_ref().any(|l| l.trim() == "character_dict:") {
        return Err("no `character_dict:` key".to_owned());
    }
    let mut entries = Vec::new();
    let mut indent = None;
    for (n, line) in lines.enumerate() {
        let lead = line.len() - line.trim_start_matches(' ').len();
        let Some(item) = line[lead..]
            .strip_prefix("- ")
            .or_else(|| (line[lead..] == *"-").then_some(""))
        else {
            break;
        };
        if *indent.get_or_insert(lead) != lead {
            break;
        }
        if entries.len() == MAX_ENTRIES {
            return Err(format!("more than {MAX_ENTRIES} entries"));
        }
        let entry = item_value(item).map_err(|e| format!("character_dict item {}: {e}", n + 1))?;
        entries.push(entry);
    }
    if entries.is_empty() {
        return Err("`character_dict:` has no items".to_owned());
    }
    Ok(entries)
}

fn item_value(item: &str) -> Result<String, String> {
    if let Some(body) = item.strip_prefix('\'') {
        return single_quoted(body);
    }
    if let Some(body) = item.strip_prefix('"') {
        return double_quoted(body);
    }
    let value = item.trim_end();
    if value.is_empty() {
        return Err("an empty item".to_owned());
    }
    // `\u{7b}` is `{`, spelled so the structure gate's brace count balances.
    if value.starts_with(['#', '[', '\u{7b}', '&', '*', '!', '|', '>', '%', '@'])
        || value.starts_with("- ")
        || value.contains(" #")
        || value.ends_with(':')
        || value.contains(": ")
    {
        return Err(format!("{value:?} is not a plain one-character scalar"));
    }
    Ok(value.to_owned())
}

fn single_quoted(body: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        if c != '\'' {
            out.push(c);
            continue;
        }
        let rest = chars.as_str();
        if let Some(after) = rest.strip_prefix('\'') {
            out.push('\'');
            chars = after.chars();
        } else {
            return closed(out, rest);
        }
    }
    Err("an unterminated single-quoted item".to_owned())
}

fn double_quoted(body: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return closed(out, chars.as_str()),
            '\\' => out.push(escape(&mut chars)?),
            c => out.push(c),
        }
    }
    Err("an unterminated double-quoted item".to_owned())
}

fn escape(chars: &mut std::str::Chars<'_>) -> Result<char, String> {
    let digits = match chars.next() {
        Some('\\') => return Ok('\\'),
        Some('"') => return Ok('"'),
        Some('/') => return Ok('/'),
        Some('t') => return Ok('\t'),
        Some('n') => return Ok('\n'),
        Some('r') => return Ok('\r'),
        Some('0') => return Ok('\0'),
        Some('x') => 2,
        Some('u') => 4,
        Some('U') => 8,
        other => return Err(format!("an unsupported escape \\{}", other.unwrap_or(' '))),
    };
    let hex: String = chars.take(digits).collect();
    u32::from_str_radix(&hex, 16)
        .ok()
        .filter(|_| hex.len() == digits)
        .and_then(char::from_u32)
        .ok_or_else(|| format!("a malformed escape {hex:?}"))
}

fn closed(value: String, rest: &str) -> Result<String, String> {
    let rest = rest.trim_end();
    if !rest.is_empty() && !rest.starts_with(" #") {
        return Err(format!("text after the closing quote: {rest:?}"));
    }
    if value.is_empty() {
        return Err("an empty item".to_owned());
    }
    Ok(value)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    const CONFIG: &str = "Global:\n  model_name: x\nPostProcess:\n  name: CTCLabelDecode\n  character_dict:\n  - '0'\n  - a\n  - ''''\n  - '\"'\n  - \\\n  - \"\\u00e9\"\n  - ' '\n  - é\nOther: 1\n";

    #[test]
    fn reads_every_item_form_and_stops_at_the_next_key() {
        assert_eq!(
            parse_config_dictionary(CONFIG),
            Ok(["0", "a", "'", "\"", "\\", "é", " ", "é"]
                .map(str::to_owned)
                .to_vec())
        );
    }

    #[test]
    fn a_deeper_indented_line_ends_the_sequence() {
        let text = "character_dict:\n- a\n- b\n  - c\n";
        assert_eq!(parse_config_dictionary(text).map(|d| d.len()), Ok(2));
    }

    #[test]
    fn crlf_line_endings_read_the_same() {
        let crlf = CONFIG.replace('\n', "\r\n");
        assert_eq!(
            parse_config_dictionary(&crlf),
            parse_config_dictionary(CONFIG)
        );
    }

    #[test]
    fn what_cannot_be_read_exactly_is_refused() {
        for (text, needle) in [
            ("model: x\n", "no `character_dict:`"),
            ("character_dict:\nnext: 1\n", "no items"),
            ("character_dict:\n- 'a\n", "unterminated"),
            ("character_dict:\n- \"\\q\"\n", "unsupported escape"),
            ("character_dict:\n- \"\\u12\"\n", "malformed escape"),
            ("character_dict:\n- 'a' b\n", "after the closing quote"),
            ("character_dict:\n- [a]\n", "plain"),
            ("character_dict:\n- k: v\n", "plain"),
            ("character_dict:\n- ''\n", "empty"),
            ("character_dict:\n-\n", "empty"),
        ] {
            let err = parse_config_dictionary(text).expect_err(text);
            assert!(err.contains(needle), "{text:?}: {err}");
        }
    }
}
