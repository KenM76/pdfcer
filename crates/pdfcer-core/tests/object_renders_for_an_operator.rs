//! `Object` and `Name` render as one operator-facing line (`Pass 296.2`).
//!
//! # What this defends
//!
//! A consuming shell showing a duplicate-key anomaly needs to print `kept` and
//! `discarded` in a sentence. With no `Display` it had two options — `{:?}`,
//! which puts Rust syntax and an unbounded object graph in front of a CAD
//! operator, or a local renderer. It wrote the renderer, 26 lines and a match
//! arm per kind, and filed the fact as a boundary finding rather than a
//! complaint.
//!
//! Three things were diverging across consumers and **none of them would fail
//! a test**: whether a name carries its slash, whether a container expands,
//! and the `#[non_exhaustive]` wildcard — a shell's catch-all renders "a value
//! this build does not recognise", so adding a variant here would quietly make
//! every shell say that about it.
//!
//! # ★ The contract this pins
//!
//! **Scalars exact, containers named rather than expanded.** The second half
//! is the one a future change will be tempted to "improve": expanding a
//! container makes the small case prettier and the large case unreadable, and
//! the pair being compared is `kept` versus `discarded` on one line.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use pdfcer_core::object::{Dict, Name, ObjId, Object};

fn name(s: &str) -> Name {
    Name(s.as_bytes().to_vec())
}

#[test]
fn a_name_renders_with_its_slash() {
    assert_eq!(name("PageMode").to_string(), "/PageMode");
    assert_eq!(Object::Name(name("Type")).to_string(), "/Type");
}

#[test]
fn a_name_that_is_not_plain_ascii_is_escaped_not_lossy() {
    // §7.3.5 NOTE 1: `/A#20B` and a literal space are one name. It renders in
    // the escaped form, which is the one that can be typed back.
    assert_eq!(Name(b"A B".to_vec()).to_string(), "/A#20B");
}

#[test]
fn scalars_render_exactly() {
    assert_eq!(Object::Null.to_string(), "null");
    assert_eq!(Object::Boolean(true).to_string(), "true");
    assert_eq!(Object::Integer(12).to_string(), "12");
    assert_eq!(Object::Real(1.5).to_string(), "1.5");
    assert_eq!(Object::Reference(ObjId::new(12, 0)).to_string(), "12 0 R");
}

#[test]
fn a_printable_string_reads_as_text_and_anything_else_as_hex() {
    assert_eq!(Object::String(b"Hello".to_vec()).to_string(), "(Hello)");
    // Not printable: rendered as the hex form the file itself would use,
    // rather than through `from_utf8_lossy`, which would show replacement
    // characters and call them the value.
    assert_eq!(
        Object::String(vec![0x01, 0x02, 0xFF]).to_string(),
        "<0102FF>"
    );
}

#[test]
fn containers_are_named_and_never_expanded() {
    let arr = Object::Array(vec![
        Object::Integer(1),
        Object::Integer(2),
        Object::Integer(3),
    ]);
    assert_eq!(arr.to_string(), "an array of 3 items");

    let mut d = Dict::new();
    d.0.push((name("Type"), Object::Name(name("Page"))));
    assert_eq!(Object::Dict(d).to_string(), "a dictionary of 1 entry");

    // ★ The assertion that keeps the contract honest: a container's CONTENTS
    // must not appear. Expand a container and this goes red even if the count
    // is still right.
    assert!(
        !arr.to_string().contains('1'),
        "a named container must not leak its contents: {arr}"
    );
}

#[test]
fn one_entry_is_not_one_entries() {
    let mut d = Dict::new();
    d.0.push((name("A"), Object::Null));
    assert_eq!(
        Object::Dict(d.clone()).to_string(),
        "a dictionary of 1 entry"
    );
    d.0.push((name("B"), Object::Null));
    assert_eq!(Object::Dict(d).to_string(), "a dictionary of 2 entries");
    assert_eq!(
        Object::Array(vec![Object::Null]).to_string(),
        "an array of 1 item"
    );
}

#[test]
fn display_is_not_debug() {
    // Debug keeps the full graph and has NOT changed; the two must not be
    // allowed to converge, because the whole point of Display is that it fits
    // on a line Debug never could.
    let mut d = Dict::new();
    for i in 0..14 {
        d.0.push((name(&format!("K{i}")), Object::Integer(i)));
    }
    let o = Object::Dict(d);
    assert_eq!(o.to_string(), "a dictionary of 14 entries");
    assert!(
        format!("{o:?}").len() > o.to_string().len() * 2,
        "Debug must still be the full graph"
    );
}
