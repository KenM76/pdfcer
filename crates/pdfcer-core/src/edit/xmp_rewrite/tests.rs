#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)] // a panic here is the test failing

use super::*;

const DC: &str = "http://purl.org/dc/elements/1.1/";
const PDFX: &str = "http://ns.adobe.com/pdfx/1.3/";
const PDF: &str = "http://ns.adobe.com/pdf/1.3/";

const HEAD: &str = "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n";
const TAIL: &str = " </rdf:RDF>\n</x:xmpmeta>\n                    \n<?xpacket end=\"w\"?>";

fn packet(descs: &str) -> String {
    format!("{HEAD}{descs}{TAIL}")
}

fn title() -> Property {
    Property {
        ns: DC,
        prefix: "dc",
        local: "title".into(),
        shape: Shape::LangAlt,
    }
}

fn creator() -> Property {
    Property {
        ns: DC,
        prefix: "dc",
        local: "creator".into(),
        shape: Shape::Seq,
    }
}

fn simple(ns: &'static str, prefix: &'static str, local: &str) -> Property {
    Property {
        ns,
        prefix,
        local: local.into(),
        shape: Shape::Simple,
    }
}

fn policy(languages: AltLanguages) -> Policy {
    Policy {
        languages,
        authors: AuthorItems::Whole,
    }
}

fn set(text: &str, prop: &Property, value: Option<&str>) -> String {
    match rewrite(text, prop, value, policy(AltLanguages::ReplaceDuplicates)).unwrap() {
        Change::Rewritten(t) => t,
        Change::InStep => panic!("expected a rewrite"),
    }
}

/// Every `rdf:li` of `prop` as `(xml:lang, text)`, or the simple value as
/// one item with no language.
fn read(text: &str, ns: &str, local: &str) -> Vec<(Option<String>, String)> {
    let doc = Document::parse(text).unwrap();
    let rdf = rdf_root(&doc).unwrap();
    for desc in descriptions(rdf) {
        if let Some(v) = desc.attribute((ns, local)) {
            return vec![(None, v.to_owned())];
        }
        if let Some(el) = desc.children().find(|n| is(*n, ns, local)) {
            let items: Vec<_> = el
                .descendants()
                .filter(|n| is(*n, RDF_NS, "li"))
                .map(|li| {
                    (
                        li.attribute((XML_NS, "lang")).map(str::to_owned),
                        li.text().unwrap_or("").to_owned(),
                    )
                })
                .collect();
            if items.is_empty() {
                return vec![(None, el.text().unwrap_or("").to_owned())];
            }
            return items;
        }
    }
    Vec::new()
}

const MULTI_TITLE: &str = "  <rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n   <dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">Old</rdf:li><rdf:li xml:lang=\"en-US\">Old</rdf:li><rdf:li xml:lang=\"de\">Alt</rdf:li></rdf:Alt></dc:title>\n  </rdf:Description>\n";

fn langs(text: &str) -> Vec<(Option<String>, String)> {
    read(text, DC, "title")
}

fn item(lang: &str, text: &str) -> (Option<String>, String) {
    (Some(lang.to_owned()), text.to_owned())
}

#[test]
fn title_replaces_x_default_and_its_duplicate_by_default() {
    let out = set(&packet(MULTI_TITLE), &title(), Some("New"));
    assert_eq!(langs(&out), [item("x-default", "New"), item("de", "Alt")]);
}

#[test]
fn title_language_policies_keep_and_drop() {
    let text = packet(MULTI_TITLE);
    let run = |languages| match rewrite(&text, &title(), Some("New"), policy(languages)).unwrap() {
        Change::Rewritten(t) => langs(&t),
        Change::InStep => panic!("expected a rewrite"),
    };
    assert_eq!(
        run(AltLanguages::Keep),
        [
            item("x-default", "New"),
            item("en-US", "Old"),
            item("de", "Alt")
        ]
    );
    assert_eq!(run(AltLanguages::Drop), [item("x-default", "New")]);
}

#[test]
fn only_the_property_bytes_change() {
    let text = packet(MULTI_TITLE);
    let out = set(&text, &title(), Some("New"));
    let start = text.find("<dc:title>").unwrap();
    assert_eq!(&out[..start], &text[..start]);
    let old_end = text.find("</dc:title>").unwrap() + "</dc:title>".len();
    let new_end = out.find("</dc:title>").unwrap() + "</dc:title>".len();
    assert_eq!(
        &out[new_end..],
        &text[old_end..],
        "wrapper and padding kept"
    );
}

#[test]
fn a_value_already_there_is_in_step() {
    let text = packet(MULTI_TITLE);
    let r = rewrite(&text, &title(), Some("Old"), policy(AltLanguages::Drop)).unwrap();
    assert_eq!(r, Change::InStep);
    let r = rewrite(
        &text,
        &simple(PDFX, "pdfx", "Absent"),
        None,
        policy(AltLanguages::Keep),
    );
    assert_eq!(r.unwrap(), Change::InStep);
}

#[test]
fn a_missing_custom_key_is_inserted_with_its_namespace_declared() {
    let text = packet(MULTI_TITLE);
    let out = set(
        &text,
        &simple(PDFX, "pdfx", "Department"),
        Some("A & B <c>"),
    );
    assert!(out.contains("xmlns:pdfx=\"http://ns.adobe.com/pdfx/1.3/\""));
    assert_eq!(
        read(&out, PDFX, "Department"),
        [(None, "A & B <c>".to_owned())]
    );
    assert_eq!(langs(&out), langs(&text), "the title is untouched");
    assert!(
        out.contains("\n   <pdfx:Department"), // string-gap-exempt: XML indentation under test
        "indented like its sibling"
    );
}

#[test]
fn a_bound_prefix_is_reused() {
    let desc = "  <rdf:Description rdf:about=\"\" xmlns:my=\"http://ns.adobe.com/pdfx/1.3/\"/>\n";
    let out = set(&packet(desc), &simple(PDFX, "pdfx", "K"), Some("v"));
    assert!(out.contains("<my:K>v</my:K></rdf:Description>"), "{out}");
}

#[test]
fn a_packet_with_no_description_gets_one() {
    let out = set(&packet(""), &title(), Some("T"));
    assert_eq!(langs(&out), [item("x-default", "T")]);
    assert!(out.contains("<rdf:Description rdf:about=\"\">"));
}

#[test]
fn an_attribute_form_value_is_replaced_in_place() {
    let desc = "  <rdf:Description rdf:about=\"\" xmlns:pdf=\"http://ns.adobe.com/pdf/1.3/\" pdf:Keywords=\"old\"/>\n";
    let text = packet(desc);
    let out = set(&text, &simple(PDF, "pdf", "Keywords"), Some("a \"b\""));
    assert_eq!(out, text.replace("\"old\"", "\"a &quot;b&quot;\""));
    assert_eq!(read(&out, PDF, "Keywords"), [(None, "a \"b\"".to_owned())]);
}

#[test]
fn removal_takes_the_element_or_attribute_and_its_leading_space() {
    let desc = "  <rdf:Description rdf:about=\"\" xmlns:pdf=\"http://ns.adobe.com/pdf/1.3/\" pdf:Keywords=\"k\"/>\n";
    let out = set(&packet(desc), &simple(PDF, "pdf", "Keywords"), None);
    assert_eq!(out, packet(desc).replace(" pdf:Keywords=\"k\"", ""));
    let out = set(&packet(MULTI_TITLE), &title(), None);
    assert!(read(&out, DC, "title").is_empty());
    assert!(out.contains("xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n  </rdf:Description>"));
}

#[test]
fn author_is_one_item_or_split_by_setting() {
    let desc = "  <rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:creator><rdf:Seq><rdf:li>A</rdf:li><rdf:li>B</rdf:li></rdf:Seq></dc:creator></rdf:Description>\n";
    let text = packet(desc);
    let out = set(&text, &creator(), Some("Ann; Bob"));
    assert_eq!(read(&out, DC, "creator"), [(None, "Ann; Bob".to_owned())]);
    let split = Policy {
        languages: AltLanguages::Keep,
        authors: AuthorItems::SplitOnSemicolons,
    };
    let Change::Rewritten(out) = rewrite(&text, &creator(), Some("Ann; Bob"), split).unwrap()
    else {
        panic!("expected a rewrite");
    };
    let names: Vec<_> = read(&out, DC, "creator")
        .into_iter()
        .map(|(_, t)| t)
        .collect();
    assert_eq!(names, ["Ann", "Bob"]);
    assert_eq!(
        rewrite(&out, &creator(), Some("Ann;Bob"), split).unwrap(),
        Change::InStep
    );
}

#[test]
fn refusals_are_named() {
    let text = packet(MULTI_TITLE);
    let p = policy(AltLanguages::Keep);
    assert_eq!(
        rewrite(&text, &title(), Some("a\u{1}b"), p),
        Err(Failure::Unrepresentable)
    );
    assert_eq!(
        rewrite("<not xml", &title(), Some("t"), p),
        Err(Failure::Unreadable)
    );
    assert_eq!(
        rewrite("<a/>", &title(), Some("t"), p),
        Err(Failure::Unreadable)
    );
    assert!(
        Packet::read(b"\xFF\xFE<\x00").is_err(),
        "UTF-16 is reported"
    );
}

#[test]
fn a_bom_is_kept() {
    let p = Packet::read(b"\xEF\xBB\xBF<a/>").unwrap();
    assert!(p.bom);
    assert_eq!(p.text, "<a/>");
    assert_eq!(p.encode("<b/>"), b"\xEF\xBB\xBF<b/>");
}

#[test]
fn pdfa_part_reads_attribute_and_element_forms() {
    let ns = "xmlns:pdfaid=\"http://www.aiim.org/pdfa/ns/id/\"";
    let attr = packet(&format!(
        "<rdf:Description rdf:about=\"\" {ns} pdfaid:part=\"2\"/>"
    ));
    assert_eq!(pdfa_part(&attr), Ok(Some(2)));
    let elem = packet(&format!(
        "<rdf:Description rdf:about=\"\" {ns}><pdfaid:part>1</pdfaid:part></rdf:Description>"
    ));
    assert_eq!(pdfa_part(&elem), Ok(Some(1)));
    assert_eq!(pdfa_part(&packet(MULTI_TITLE)), Ok(None));
}

#[test]
fn keys_are_escaped_to_ncnames() {
    assert_eq!(escape_key("My Key"), "My\u{2182}0020Key");
    assert_eq!(escape_key("1st"), "\u{2182}0031st");
    assert_eq!(escape_key("a:b"), "a\u{2182}003Ab");
    assert_eq!(escape_key("x\u{1F600}"), "x\u{2182}D83D\u{2182}DE00");
    assert_eq!(escape_key("Caf\u{E9}-1.0"), "Caf\u{E9}-1.0");
}
