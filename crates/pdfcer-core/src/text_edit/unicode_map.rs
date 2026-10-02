//! A font's `/ToUnicode` CMap as decision 172 extends it: an added code
//! either already reads back as its character or gains a `bfchar` entry
//! (ISO 32000-2 §9.10.3), written before `endcmap` with the rest verbatim.

use crate::object::{Dict, Name, ObjId, Object};
use crate::text_extract::cmap::ToUnicodeCMap;
use crate::view::DocumentView;

/// A CMap operator block holds at most 100 entries (Adobe TN 5014 §8.3).
const BLOCK_LIMIT: usize = 100;

/// The font's `/ToUnicode` stream, rewritten in place, unfiltered.
pub(crate) struct UnicodeMap {
    pub(crate) id: ObjId,
    dict: Dict,
    decoded: Vec<u8>,
    pub(crate) cmap: ToUnicodeCMap,
    /// Bytes per code: 1 for a simple font, 2 for `Identity-H`.
    code_bytes: u8,
}

impl UnicodeMap {
    /// The map `font` names, which must be a stream whose codespace is
    /// `code_bytes` wide. `Ok(None)` when the font has none.
    pub(crate) fn read(
        doc: &DocumentView<'_>,
        font: &Dict,
        code_bytes: u8,
    ) -> Result<Option<Self>, &'static str> {
        let Some(entry) = font.get(b"ToUnicode") else {
            return Ok(None);
        };
        let id = entry
            .as_reference()
            .ok_or("the /ToUnicode map is not a separate stream")?;
        let Some(Object::Stream(s)) = doc.graph().value(id) else {
            return Err("the /ToUnicode map is not a stream");
        };
        let decoded = doc
            .slice(s.data_span)
            .and_then(|raw| crate::filters::decode_stream(&s.dict, raw).ok())
            .ok_or("the /ToUnicode map could not be read")?;
        let cmap = ToUnicodeCMap::parse(&decoded);
        if cmap.codespace_widths() != [code_bytes] || endcmap_at(&decoded).is_none() {
            return Err(if code_bytes == 1 {
                "the /ToUnicode map is not a single-byte CMap pdfcer can extend"
            } else {
                "the /ToUnicode map is not a two-byte CMap pdfcer can extend"
            });
        }
        Ok(Some(Self {
            id,
            dict: s.dict.clone(),
            decoded,
            cmap,
            code_bytes,
        }))
    }

    /// Whether `code` needs a new entry to read back as `ch`.
    pub(crate) fn needs_entry(&self, ch: char, code: u32) -> Result<bool, String> {
        match self.cmap.lookup(code) {
            None => Ok(true),
            Some(s) if s.chars().eq(std::iter::once(ch)) => Ok(false),
            Some(s) => Err(format!(
                "code {code} already reads as {s:?} in the font's /ToUnicode map"
            )),
        }
    }

    /// The stream's new dictionary and bytes: `bfchar` blocks before
    /// `endcmap`, the rest verbatim.
    pub(crate) fn extended(&self, entries: &[(u32, char)]) -> (Dict, Vec<u8>) {
        let at = endcmap_at(&self.decoded).unwrap_or(self.decoded.len());
        let digits = usize::from(self.code_bytes) * 2;
        let mut block = String::new();
        for chunk in entries.chunks(BLOCK_LIMIT) {
            block.push_str(&format!("{} beginbfchar\n", chunk.len()));
            for &(code, ch) in chunk {
                let mut units = [0u16; 2];
                let hex: String = ch
                    .encode_utf16(&mut units)
                    .iter()
                    .map(|u| format!("{u:04X}"))
                    .collect();
                block.push_str(&format!("<{code:0digits$X}> <{hex}>\n"));
            }
            block.push_str("endbfchar\n");
        }
        let (head, tail) = self.decoded.split_at(at);
        let bytes = [head, block.as_bytes(), tail].concat();
        let mut dict = self.dict.clone();
        dict.remove(b"Filter");
        dict.remove(b"DecodeParms");
        let len = i64::try_from(bytes.len()).unwrap_or(i64::MAX);
        dict.insert(Name(b"Length".to_vec()), Object::Integer(len));
        (dict, bytes)
    }

    /// The map as it reads once `entries` are added.
    pub(crate) fn with_entries(&self, entries: &[(u32, char)]) -> ToUnicodeCMap {
        ToUnicodeCMap::parse(&self.extended(entries).1)
    }
}

/// Offset of the last `endcmap` keyword.
fn endcmap_at(cmap: &[u8]) -> Option<usize> {
    cmap.windows(7).rposition(|w| w == b"endcmap")
}
