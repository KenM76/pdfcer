//! The requirements only a person can decide (Matterhorn Protocol 1.1's
//! "human" checkpoints, and their ISO 14289-2 carry-forwards), each as a
//! question for the reviewer.

use super::{DecidedBy, FindingKind, FindingTarget, Out, PdfuaPart, requirements};

/// Push one needs-a-person finding per sourced person row of `part`.
pub(super) fn push_all(part: PdfuaPart, out: &mut Out) {
    for r in requirements(part) {
        if r.decided_by != DecidedBy::Person || !r.sourced {
            continue;
        }
        let question = r.matterhorn.map_or("", question);
        let message = format!("A person must check ({}): {question}", r.clause);
        out.push(
            FindingKind::NeedsPerson,
            r.id,
            r.id,
            None,
            FindingTarget::Document,
            message,
        );
    }
}

/// The reviewer's question for a Matterhorn human checkpoint.
fn question(checkpoint: &str) -> &'static str {
    match checkpoint {
        "01-001" => "is anything tagged as content actually decoration (an artifact)?",
        "01-002" => "is anything marked as an artifact actually meaningful content?",
        "01-006" => "does every tag's type and attributes fit what the content is?",
        "02-002" => "does each custom tag map to a standard type that fits its meaning?",
        "03-001" => "does any action make the screen flicker?",
        "03-002" => "does any embedded media flicker?",
        "03-003" => "does any script make the screen flicker?",
        "04-001" => "is any meaning carried only by colour, contrast, format or layout?",
        "05-001" => "does every media item's sound have a text alternative?",
        "05-002" => "does every sound item have a text alternative?",
        "05-003" => "does any script that beeps also notify in another way?",
        "06-004" => "does the title identify this document?",
        "08-001" => "is recognised (OCR) text free of significant errors?",
        "08-002" => "is recognised (OCR) text tagged?",
        "09-001" => "do the tags follow the logical reading order?",
        "09-002" => "is every tag nested sensibly (no table inside a heading, say)?",
        "09-003" => "does each tag's type, after role mapping, fit its content?",
        "11-007" => "is each declared language the language actually used?",
        "12-001" => "are stretched or spaced-out characters represented as their text?",
        "13-001" => "is every meaningful graphic tagged as a figure?",
        "13-002" => "does a link drawn over a meaningful image describe both?",
        "13-003" => "is every caption tagged as a caption?",
        "13-005" => "would alternate text suit a figure better than replacement text?",
        "13-006" => "are graphics that only mean something together tagged together?",
        "13-007" => "is a more accessible form of each figure available and used?",
        "13-008" => "does a figure meant to be read as text carry replacement text?",
        "14-001" => "is every heading tagged as a heading?",
        "14-004" => "do custom heading tags map to numbered headings correctly?",
        "14-005" => "do headings deeper than level 6 use H7 and below?",
        "15-001" => "is every row header tagged as a header cell?",
        "15-002" => "is every column header tagged as a header cell?",
        "15-004" => "is everything tagged as a table really rows and columns?",
        "15-005" => "can every table cell's header be found unambiguously?",
        "16-001" => "does every ordered list declare its numbering?",
        "16-002" => "does every ordered list's numbering use a recognised style?",
        "16-003" => "is every list tagged as a list?",
        "17-001" => "is every mathematical expression tagged as a formula?",
        "18-001" => "are running headers and footers marked as pagination artifacts?",
        "18-002" => "are header and footer artifacts marked as Header or Footer?",
        "19-001" => "are footnotes and endnotes tagged as notes?",
        "19-002" => "are references tagged as references?",
        "22-001" => "do article threads follow the logical reading order?",
        "24-001" => "are non-interactive form fields tagged as print fields?",
        "28-001" => "is every annotation in the right place in the reading order?",
        "28-003" => "is an annotation used for appearance tagged by what it means?",
        "28-013" => "is an image map's function available another way?",
        "29-001" => "does any script demand keystrokes at a particular speed?",
        "31-010" => "may every embedded font legally be embedded for any use?",
        _ => "review this requirement against the clause.",
    }
}
