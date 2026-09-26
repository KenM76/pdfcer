//! Every module the facade re-exports still resolves at its
//! `pdfcer_core::<module>` path, as seen from outside the crate (the view a
//! consumer such as `pdfcer-gui` has). A failed import is a compile error,
//! so this file needs no assertions; the allow only silences "unused".

#[allow(unused_imports)]
use pdfcer_core::{
    HEADER_SCAN_WINDOW, PdfError, PdfVersion, color, content, crypto, document, filters,
    font_embed, fontdata, fontinfo, function, graph, image_codec, lexer, linearization, linebreak,
    object, objstm, page_tree, parser, probe_cos_header, probe_file, probe_header, recover, span,
    text_extract, text_state, textstring, trust_chain, trust_store, vartext, view, writer, xref,
};

#[test]
fn facade_paths_resolve() {}
