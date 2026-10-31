//! Filling an XFA form field ([`EditSession::fill_xfa_field`]).

use super::{Command, CommandKind, EditError, EditSession, ObjectWrite};
use crate::graph::ObjectGraph;
use crate::object::{Name, Object, Stream};
use crate::xfa::{
    MergeOptions, XfaFillError, XfaFillOutcome, merge_data, plan_fill, read_template,
    read_xdp_sources,
};

/// The UTF-8 byte-order mark a rewritten first packet stream keeps.
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

impl EditSession {
    /// Write `value` into the XFA field (or exclusion group) at template
    /// `path` (child indices from the `template` element, as
    /// [`crate::xfa::LayoutItem::path`]). One undo entry; committed at Save.
    ///
    /// The value goes into the data node the field is bound to under `opts`
    /// (XFA 3.3 Ch.9 p.343), so it reads back through
    /// [`crate::xfa::merge_data`] and any other container sharing that node
    /// ([`XfaFillOutcome::shared_with`]) shows it too. Only the `datasets`
    /// text of that node changes; every other byte of the XDP is kept. The
    /// rewritten packet stream is FlateDecode when it had any filter, and
    /// unfiltered otherwise. `value` is XML-escaped; it is written as given,
    /// with no picture-clause formatting.
    ///
    /// # Errors
    ///
    /// [`EditError::XfaFill`] with:
    /// - [`XfaFillError::NoXfa`]: no XFA form;
    /// - [`XfaFillError::Unbound`]: nothing at `path` took a data value;
    /// - [`XfaFillError::Utf16`]: the XDP is UTF-16;
    /// - [`XfaFillError::SplitAcrossStreams`] or
    ///   [`XfaFillError::DirectStream`]: the value's packet stream cannot be
    ///   rewritten on its own;
    /// - [`XfaFillError::Xfa`]: the XDP does not read.
    pub fn fill_xfa_field(
        &mut self,
        path: &[usize],
        value: &str,
        opts: &MergeOptions,
    ) -> Result<XfaFillOutcome, EditError> {
        let (id, dict, bytes, data, shared_with) = {
            let view = self.view();
            let (xdp, sources) = read_xdp_sources(&view)
                .map_err(XfaFillError::from)?
                .ok_or(XfaFillError::NoXfa)?;
            if sources.is_empty() {
                return Err(XfaFillError::Utf16.into());
            }
            let template = read_template(&xdp)
                .map_err(XfaFillError::from)?
                .ok_or(XfaFillError::NoXfa)?;
            let merged = merge_data(&template, &xdp, opts).map_err(XfaFillError::from)?;
            let (edit, data) = plan_fill(&xdp, &merged, path, value, opts)?;
            let source = sources
                .iter()
                .find(|s| s.text.start <= edit.range.start && edit.range.end <= s.text.end)
                .ok_or(XfaFillError::SplitAcrossStreams)?;
            let id = source.id.ok_or(XfaFillError::DirectStream)?;
            let reference = Object::Reference(id);
            let Object::Stream(stream) = view.resolve(&reference) else {
                return Err(XfaFillError::DirectStream.into());
            };
            let text = xdp.text();
            let mut bytes = Vec::with_capacity(source.text.len() + edit.text.len() + 3);
            if source.bom {
                bytes.extend_from_slice(UTF8_BOM);
            }
            let slice = |r: std::ops::Range<usize>| text.get(r).unwrap_or("").as_bytes();
            bytes.extend_from_slice(slice(source.text.start..edit.range.start));
            bytes.extend_from_slice(edit.text.as_bytes());
            bytes.extend_from_slice(slice(edit.range.end..source.text.end));
            let shared_with = merged
                .bound
                .iter()
                .filter(|b| b.data == data && b.path != path)
                .map(|b| b.path.clone())
                .collect();
            (id, stream.dict.clone(), bytes, data, shared_with)
        };
        let after = self.xfa_packet_stream(dict, &bytes);
        let objects = vec![ObjectWrite {
            id,
            before: self.state.get(&id).cloned(),
            after: Some(Object::Stream(after)),
        }];
        self.commit(Command {
            kind: CommandKind::FillXfaField,
            objects,
            removals: Vec::new(),
            trailer: None,
        });
        Ok(XfaFillOutcome {
            stream: id,
            data,
            shared_with,
        })
    }

    /// `dict`'s stream carrying `decoded`, FlateDecode if it had any filter.
    fn xfa_packet_stream(&mut self, mut dict: crate::object::Dict, decoded: &[u8]) -> Stream {
        let filtered = dict.get(b"Filter").is_some();
        for key in [&b"Filter"[..], b"DecodeParms", b"DL"] {
            dict.remove(key);
        }
        let encoded = filtered
            .then(|| super::flate_encode_for_save(decoded))
            .flatten();
        let data = match &encoded {
            Some(e) => {
                dict.insert(
                    Name::from(b"Filter"),
                    Object::Name(Name::from(b"FlateDecode")),
                );
                e.as_slice()
            }
            None => decoded,
        };
        dict.insert(
            Name::from(b"Length"),
            Object::Integer(i64::try_from(data.len()).unwrap_or(i64::MAX)),
        );
        Stream {
            dict,
            data_span: self.stage_bytes(data),
        }
    }
}
