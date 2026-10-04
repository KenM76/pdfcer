//! The licence notice attached beside any bundled standard-14 face pdfcer embeds.

/// The filename the bundled-font licence notice is attached under.
///
/// Prefixed so it sorts and reads as tooling output rather than as one of the
/// operator's own attachments, and named for what it IS rather than for pdfcer,
/// because the person who opens the PDF later may have never heard of pdfcer
/// and needs to know at a glance why a licence file is in their document.
pub(crate) const BUNDLED_FONT_NOTICE_NAME: &str = "FONT-LICENSE-NOTICE.txt";

/// The BSD-3-Clause notice for pdfcer's bundled standard-14 substitute faces.
///
/// # Why this text is not paraphrased, summarised, or regenerated
///
/// BSD-3-Clause's condition is specifically that the copyright notice, "this
/// list of conditions" and "the following disclaimer" be REPRODUCED. A
/// summary does not satisfy a reproduction requirement, and a plausible
/// rewording of a licence is worse than useless — it is a claim about legal
/// terms that nobody checked. The body below is lifted verbatim from
/// `crates/pdfcer-render/assets/fonts/PROVENANCE.md`, which in turn records it
/// from pdfium's own LICENSE with the comment markers stripped.
///
/// The surrounding explanation is pdfcer's, and is deliberately separated from
/// the licence text by a rule, so a reader can see where our words stop and
/// the licence begins.
pub(crate) fn bundled_font_notice() -> String {
    format!(
        "\
FONT LICENCE NOTICE
===================

This PDF contains one or more embedded font programs that were supplied by
pdfcer's own bundled set of standard-14 substitute faces, rather than by the
document's author.

They were embedded because the document named a font it did not carry, and a
program was needed so that the text displays and prints the same way
everywhere -- for example, to satisfy a print service that requires all fonts
to be embedded.

The faces come from the Chromium pdfium project and are BSD-3-Clause
licensed. That licence permits this use. It also requires that the notice
below travel with any redistribution in binary form, which is why this file
is attached to the document rather than left somewhere else.

If you redistribute this PDF, keep this attachment.

Faces that may be present: FoxitSans, FoxitSerif, FoxitFixed (each in
regular, bold, italic and bold-italic), FoxitSymbol and FoxitDingbats.

----------------------------------------------------------------------
{}
----------------------------------------------------------------------

Attached automatically by pdfcer. Source of the faces:
https://pdfium.googlesource.com/pdfium/
",
        PDFIUM_BSD_LICENSE.trim()
    )
}

/// pdfium's BSD-3-Clause text, verbatim, comment markers removed.
///
/// Kept as its own constant so the reproduction requirement is satisfied by a
/// single unbroken block that can be diffed against
/// `crates/pdfcer-render/assets/fonts/PROVENANCE.md`. Interpolating pdfcer's own
/// prose into it would make that check impossible.
pub(crate) const PDFIUM_BSD_LICENSE: &str = r#"
Copyright 2014 The PDFium Authors

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

   * Redistributions of source code must retain the above copyright
notice, this list of conditions and the following disclaimer.
   * Redistributions in binary form must reproduce the above
copyright notice, this list of conditions and the following disclaimer
in the documentation and/or other materials provided with the
distribution.
   * Neither the name of Google Inc. nor the names of its
contributors may be used to endorse or promote products derived from
this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
"#;
