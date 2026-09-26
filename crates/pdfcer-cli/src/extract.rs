use super::*;

/// Implement `pdfcer extract-text`.
///
/// ## Where the two output channels go, and why
///
/// The subcommand has two things to say — the document's text, and the
/// counters describing how much of that text is actually the document's
/// — and mixing them on one stream would make either one unparseable.
/// So:
///
/// | invocation | stdout | stderr | file |
/// |---|---|---|---|
/// | `extract-text f.pdf` | the text | the result line | — |
/// | `extract-text f.pdf -o t.txt` | the result line | — | the text |
/// | `extract-text f.pdf --json` | the JSON | — | — |
/// | `extract-text f.pdf --json -o t.json` | the result line | — | the JSON |
///
/// The rule underneath: **the result line goes to stdout unless stdout
/// is carrying the payload**, in which case it goes to stderr. A shell
/// pipeline (`extract-text f.pdf | grep …`) gets clean text and still
/// sees the honesty line on the terminal; a script that redirects with
/// `-o` gets the counters on stdout where it can read them.
///
/// The result line follows the R5 stable-line contract used by every
/// other subcommand: `key=value`, fixed order, **appended to and never
/// reordered**.
pub(crate) fn cmd_extract_text(
    input: &Path,
    pages_spec: &str,
    output: Option<&Path>,
    json: bool,
    include_artifacts: bool,
    spans: bool,
) -> u8 {
    use pdfcer_core::text_extract::{self, ExtractOptions};

    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let page_list = match pdfcer_core::page_tree::pages(&doc) {
        Ok(pages) => pages,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    let indices = match parse_pages(pages_spec, page_list.len()) {
        Ok(indices) => indices,
        Err(message) => {
            eprintln!("pdfcer: {}: --pages {message}", input.display());
            return exit::EDIT_REFUSED;
        }
    };

    // The operator's persisted word-gap ratio. Extraction heuristics are
    // the one place where a document that reads fine to a human can
    // extract wrong, so this is a knob worth honouring rather than a
    // constant worth defending.
    let (settings, settings_report) =
        pdfcer_core::settings::Settings::load(pdfcer_core::settings::resolve_store());
    report_settings(&settings_report);
    let options = ExtractOptions::default()
        .with_artifacts(include_artifacts)
        .with_word_gap_ratio(settings.word_gap_ratio)
        // The two EXTRACT-radius R169 knobs. Both move character offsets,
        // so both move what a text search and a text-based redaction
        // match (R35) — which is why they are honoured here rather than
        // left to a hard-coded constant nobody can see.
        .with_unmappable_code(settings.unmappable_code)
        .with_actual_text(settings.actual_text)
        // `--spans` (Pass 145.0). Provenance capture is what produces the
        // show-operator byte span, and it is off unless asked for: it costs
        // memory per glyph, and every existing consumer's output must stay
        // byte-for-byte what it was.
        .with_provenance(spans);
    let extracted = match text_extract::extract_pages(&doc, &indices, &options) {
        Ok(extracted) => extracted,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };

    let payload = if json {
        extraction_json(input, &extracted)
    } else {
        // A trailing newline on the text form: without it a shell prompt
        // lands mid-line after the last extracted word, and a `cat` of
        // the `-o` file runs into the next command. The JSON form already
        // ends with one.
        let mut text = extracted.plain_text();
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text
    };

    // Deliver the payload.
    match output {
        Some(path) => {
            if let Err(err) = std::fs::write(path, payload.as_bytes()) {
                eprintln!("pdfcer: {}: {err}", path.display());
                return exit::IO_ERROR;
            }
        }
        None => print!("{payload}"),
    }

    // The stable result line, on whichever stream is free.
    let d = &extracted.diagnostics;
    let line = format!(
        "extracted {} pages={} chars={} codes={} \
via_tounicode={} via_encoding={} via_cid={} via_extension={} failed={} \
sourced_pct={:.1} spaces_derived={} lines_derived={} \
actual_text={} artifacts={} reversed={} identity_no_tounicode={} \
ucs2_missing={} predefined_cmaps_missing={} tagged={} suspects={} \
struct_tree={} forms={} rtl_runs={} invisible={} unreadable_pages={} \
contents_unresolved={} type3_no_tounicode={} pages_resources_defaulted={}",
        input.display(),
        extracted.pages.len(),
        extracted.plain_text().chars().count(),
        d.codes_total,
        d.via_to_unicode,
        d.via_encoding_agl,
        d.via_cid_collection,
        d.via_glyph_name_extension,
        d.ladder_failures,
        d.sourced_fraction().unwrap_or(0.0) * 100.0,
        d.spaces_derived,
        d.lines_derived,
        d.actual_text_applied,
        d.artifact_sequences,
        d.reversed_chars_sequences,
        d.identity_fonts_without_to_unicode,
        d.ucs2_cmaps_unavailable,
        d.predefined_cmaps_unavailable,
        d.tagged,
        d.suspects,
        d.struct_tree_present,
        d.forms_executed,
        d.rtl_runs,
        d.invisible_glyphs,
        d.pages_unreadable,
        // Appended after every pre-existing key (the stable-line
        // contract's append-never-reorder rule): content streams the
        // pages named but the file does not contain, so their text is
        // missing from this extraction rather than absent from the
        // document.
        d.contents_unresolved,
        // `Pass 127.0`, appended per the same rule: Type 3 fonts with no
        // `/ToUnicode`. The simple-font twin of `identity_no_tounicode`
        // above, and the reason a document can render text this command
        // cannot extract.
        d.type3_fonts_without_to_unicode,
        // `Pass 290.0`, appended per the same rule: pages whose `/Resources`
        // was on neither the page nor any ancestor. Text extraction is
        // font-driven, so such a page has no `/Font` to resolve `Tf`
        // against — this is the single cause behind what would otherwise
        // read as a scatter of per-face failures, and the reason a page that
        // visibly holds text can extract as nothing.
        d.pages_resources_defaulted,
    );
    if output.is_some() || json {
        // stdout is free (the payload went to a file), or the payload is
        // JSON that already carries everything — either way stdout is
        // the right home for the machine-readable line. The `--json`
        // to-stdout case is the one exception: there the JSON *is* the
        // payload on stdout, so the line goes to stderr.
        if json && output.is_none() {
            eprintln!("{line}");
        } else {
            println!("{line}");
        }
    } else {
        eprintln!("{line}");
    }

    // Named diagnostics on stderr, always — they are the whole point of
    // "fuzzy, never sneaky" and a run that hid them would be the thing
    // rule 4 exists to prevent.
    for note in &d.notes {
        eprintln!("pdfcer: {note}");
    }

    exit::SUCCESS
}

/// Serialize an extraction to JSON.
///
/// Hand-rolled rather than reaching for `serde_json`, deliberately: this
/// is the only JSON surface in the whole workspace, the schema is fixed
/// here, and `docs/LEGAL.md` §6 makes every added dependency a decision
/// with a license classification and a `THIRD_PARTY_LICENSES.md`
/// regeneration behind it. Sixty lines of string building is the cheaper
/// side of that trade. Escaping goes through [`json_escape`], which is
/// the only part that can be got wrong.
///
/// ## Schema
///
/// ```jsonc
/// {
///   "input": "...",
///   "include_artifacts": false,
///   "diagnostics": { /* every TextDiagnostics counter, flat */ },
///   "notes": ["text: ..."],
///   "pages": [
///     { "page": 1,                       // 1-based
///       "runs": [
///         { "origin": "glyphs",          // glyphs | actual_text |
///                                        // derived_word_space |
///                                        // derived_line_break
///           "sourced": true,             // did this come from the FILE?
///           "text": "Hello",
///           "artifact": "pagination",    // omitted when not an artifact
///           "mcid": 0,                   // omitted when absent
///           "bbox": [llx, lly, urx, ury],
///           "glyphs": [
///             { "code": 72, "rung": "encoding_agl", "sourced": true,
///               "start": 0, "len": 1,
///               "x": 72.0, "y": 700.0, "advance": 17.3, "size": 24.0,
///               "invisible": false,
///               // the next three ONLY with `--spans`:
///               "op_start": 37, "op_len": 2, "stream": "page" }
///           ] } ] } ]
/// }
/// ```
///
/// `op_start`/`op_len`/`stream` are the **show operator** a glyph came from
/// (`Pass 145.0`), and they are the pin `format-text --pin-span` takes. They
/// appear only when `--spans` was given, because provenance capture is off by
/// default and costs memory per glyph — and because *absent* and *zero* must
/// not be the same answer. `stream` is `"page"` for the page's own
/// concatenated `/Contents` buffer and `"form:N"` for a form XObject's own
/// buffer; a span is meaningless without it, since the two are different byte
/// spaces.
///
/// The two `sourced` booleans are the schema's reason to exist. A
/// consumer that filters runs on `sourced == true` gets exactly the
/// characters the document provides; one that additionally filters
/// glyphs on `sourced == true` drops the U+FFFD the ladder could not
/// resolve.
pub(crate) fn extraction_json(
    input: &Path,
    extracted: &pdfcer_core::text_extract::ExtractedText,
) -> String {
    let d = &extracted.diagnostics;
    let mut out = String::with_capacity(4096);
    out.push_str("{\n");
    out.push_str(&format!(
        "  \"input\": \"{}\",\n",
        json_escape(&input.display().to_string())
    ));
    out.push_str(&format!(
        "  \"include_artifacts\": {},\n",
        extracted.includes_artifacts()
    ));

    out.push_str("  \"diagnostics\": {\n");
    let counters: [(&str, u64); 21] = [
        ("codes_total", d.codes_total),
        ("via_to_unicode", d.via_to_unicode),
        ("via_encoding_agl", d.via_encoding_agl),
        ("via_cid_collection", d.via_cid_collection),
        ("via_glyph_name_extension", d.via_glyph_name_extension),
        ("ladder_failures", d.ladder_failures),
        (
            "identity_fonts_without_to_unicode",
            d.identity_fonts_without_to_unicode,
        ),
        ("ucs2_cmaps_unavailable", d.ucs2_cmaps_unavailable),
        (
            "predefined_cmaps_unavailable",
            d.predefined_cmaps_unavailable,
        ),
        ("actual_text_applied", d.actual_text_applied),
        ("actual_text_suppressions", d.actual_text_suppressions),
        ("alt_entries", d.alt_entries),
        ("expansion_entries", d.expansion_entries),
        ("artifact_sequences", d.artifact_sequences),
        ("artifact_chars", d.artifact_chars),
        ("reversed_chars_sequences", d.reversed_chars_sequences),
        ("spaces_derived", d.spaces_derived),
        ("lines_derived", d.lines_derived),
        ("rtl_runs", d.rtl_runs),
        ("invisible_glyphs", d.invisible_glyphs),
        // `Pass 127.0`. Type 3 fonts with no `/ToUnicode` — the reason a
        // document can render text this extraction cannot recover.
        (
            "type3_fonts_without_to_unicode",
            d.type3_fonts_without_to_unicode,
        ),
    ];
    for (name, value) in counters {
        out.push_str(&format!("    \"{name}\": {value},\n"));
    }
    out.push_str(&format!("    \"tagged\": {},\n", d.tagged));
    out.push_str(&format!("    \"suspects\": {},\n", d.suspects));
    out.push_str(&format!(
        "    \"struct_tree_present\": {},\n",
        d.struct_tree_present
    ));
    out.push_str(&format!(
        "    \"tag_suspect_sequences\": {},\n",
        d.tag_suspect_sequences
    ));
    out.push_str(&format!("    \"forms_executed\": {},\n", d.forms_executed));
    out.push_str(&format!(
        "    \"form_depth_overflows\": {},\n",
        d.form_depth_overflows
    ));
    out.push_str(&format!(
        "    \"pages_unreadable\": {},\n",
        d.pages_unreadable
    ));
    out.push_str(&format!(
        "    \"fonts_with_estimated_widths\": {},\n",
        d.fonts_with_estimated_widths
    ));
    out.push_str(&format!(
        "    \"sourced_fraction\": {:.6}\n",
        d.sourced_fraction().unwrap_or(0.0)
    ));
    out.push_str("  },\n");

    out.push_str("  \"notes\": [");
    for (i, note) in d.notes.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!("\n    \"{}\"", json_escape(note)));
    }
    if d.notes.is_empty() {
        out.push_str("],\n");
    } else {
        out.push_str("\n  ],\n");
    }

    out.push_str("  \"pages\": [");
    for (page_index, page) in extracted.pages.iter().enumerate() {
        if page_index > 0 {
            out.push(',');
        }
        out.push_str("\n    {\n");
        out.push_str(&format!("      \"page\": {},\n", page.page_index + 1));
        out.push_str("      \"runs\": [");
        for (run_index, run) in page.runs.iter().enumerate() {
            if run_index > 0 {
                out.push(',');
            }
            out.push_str("\n        {");
            out.push_str(&format!("\"origin\": \"{}\", ", run.origin.as_str()));
            out.push_str(&format!("\"sourced\": {}, ", run.is_sourced()));
            out.push_str(&format!("\"text\": \"{}\"", json_escape(&run.text)));
            if let Some(artifact) = &run.artifact {
                out.push_str(&format!(
                    ", \"artifact\": \"{}\"",
                    json_escape(artifact.as_str())
                ));
            }
            if let Some(mcid) = run.mcid {
                out.push_str(&format!(", \"mcid\": {mcid}"));
            }
            if let Some(b) = run.bbox {
                out.push_str(&format!(
                    ", \"bbox\": [{:.2}, {:.2}, {:.2}, {:.2}]",
                    b.llx, b.lly, b.urx, b.ury
                ));
            }
            if !run.glyphs.is_empty() {
                // `Pass 139.0`: the run's writing direction, published once
                // per run rather than only per glyph because every glyph in
                // a run shares it by construction (`layout` closes a run on
                // a direction change) and because a consumer orienting an
                // I-beam or building `/QuadPoints` wants one answer per
                // selectable unit. Omitted for a glyph-less run — derived
                // whitespace and `/ActualText` have no baseline, and
                // printing `[1, 0]` there would be inventing one.
                out.push_str(&format!(
                    ", \"direction\": [{:.4}, {:.4}]",
                    run.direction().0,
                    run.direction().1
                ));
                out.push_str(", \"glyphs\": [");
                for (i, g) in run.glyphs.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    // `direction` rides beside `advance` and `size` because
                    // those two are the MAGNITUDES of the same two basis
                    // vectors, and a consumer reading them without it has no
                    // choice but to assume the text ran along +x. `[1, 0]`
                    // for ordinary horizontal text, so an existing
                    // consumer's parse and behaviour are both unchanged.
                    out.push_str(&format!(
                        "{{\"code\": {}, \"rung\": \"{}\", \"sourced\": {}, \
\"start\": {}, \"len\": {}, \"x\": {:.2}, \"y\": {:.2}, \"advance\": {:.2}, \
\"size\": {:.2}, \"direction\": [{:.4}, {:.4}], \"invisible\": {}",
                        g.code,
                        g.rung.as_str(),
                        g.rung.is_sourced(),
                        g.text_start,
                        g.text_len,
                        g.x,
                        g.y,
                        g.advance,
                        g.size,
                        g.direction.0,
                        g.direction.1,
                        g.invisible
                    ));
                    // `Pass 145.0`. Emitted only when provenance was
                    // captured (`--spans`), so the default schema is
                    // byte-for-byte what it was — and so a consumer can tell
                    // "not captured" from "no span", which a zero could not.
                    //
                    // `stream` rides beside the span because a span is
                    // MEANINGLESS without the buffer it indexes: a page's
                    // /Contents are concatenated into one decoded buffer, and
                    // every form XObject is a separate one with its own
                    // offsets. A consumer that pinned a form's span against
                    // the page's buffer would name a different operator, or
                    // none.
                    if let Some(prov) = g.provenance.as_ref() {
                        let stream = match prov.content_stream {
                            pdfcer_core::text_extract::ContentStreamRef::Form { object } => {
                                format!("\"form:{object}\"")
                            }
                            _ => "\"page\"".to_owned(),
                        };
                        out.push_str(&format!(
                            ", \"op_start\": {}, \"op_len\": {}, \"stream\": {stream}",
                            prov.operator_span.start, prov.operator_span.len
                        ));
                    }
                    out.push('}');
                }
                out.push(']');
            }
            out.push('}');
        }
        out.push_str(if page.runs.is_empty() {
            "]\n"
        } else {
            "\n      ]\n"
        });
        out.push_str("    }");
    }
    out.push_str(if extracted.pages.is_empty() {
        "]\n"
    } else {
        "\n  ]\n"
    });
    out.push_str("}\n");
    out
}

/// `run-repertoire` (`Pass 280.0`): which characters one located run accepts.
///
/// The scriptable form of the question a GUI asks to grey a key before the
/// operator presses it. The CLI has no session and no undo, so the invocation
/// IS the answer — and the answer is the same one `edit-text` would enforce,
/// because the query asks the accepting code rather than describing it.
///
/// `--list` prints the characters; without it the line carries the counts,
/// which is what a script branching on "can I edit this run at all?" needs.
pub(crate) fn cmd_run_repertoire(
    input: &Path,
    page: usize,
    find: &str,
    pin_span: Option<&str>,
    list: bool,
) -> u8 {
    if page == 0 {
        eprintln!("pdfcer: --page is 1-based; 0 is not a valid page number");
        return exit::EDIT_REFUSED;
    }
    let pin = match pin_span {
        Some(spec) => match parse_pin_span(spec) {
            Ok(span) => Some(span),
            Err(msg) => {
                eprintln!("pdfcer: {msg}");
                return exit::EDIT_REFUSED;
            }
        },
        None => None,
    };
    // Same boundary refusal `font-preflight` applies, and for the same reason:
    // core would answer about whichever run it located first, which is not a
    // question anybody meant to ask. Naming the FLAG is something core cannot
    // do.
    if find.is_empty() && pin.is_none() {
        eprintln!(
            "pdfcer: run-repertoire needs --find TEXT, or --pin-span START:LEN with an \
             empty --find to mean the whole pinned show operator"
        );
        return exit::EDIT_REFUSED;
    }
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let session = pdfcer_core::edit::EditSession::new(doc);
    let rep = match session.run_repertoire(page - 1, find, pin) {
        Ok(rep) => rep,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::EDIT_REFUSED;
        }
    };

    // The disclosure first, because it is the half that cannot be seen in a
    // count: an empty set with a reason is a run an editor should decline to
    // open, and it reads identically to "this font has nothing" without it.
    if let Some(reason) = &rep.reason {
        eprintln!(
            "pdfcer: {}: this run accepts NOTHING -- {reason}",
            input.display()
        );
    }
    if rep.embedded_subset {
        eprintln!(
            "pdfcer: {}: its font is an embedded SUBSET, so the answer is what this FILE carries, not what the face could draw. `format-text --set-font` is the remedy for a character it lacks.",
            input.display()
        );
    }

    let chars = if list {
        // CODE POINTS, NOT THE CHARACTERS THEMSELVES, and the first cut
        // printed the characters. `sanitize_token` maps a space to `_`, so
        // the set {space} and the set {underscore} printed IDENTICALLY -- and
        // a set containing a comma, a quote or a newline is worse. A shell
        // uses the API; a script uses this line, and it must be unambiguous.
        let list: Vec<String> = rep
            .accepted
            .iter()
            .map(|ch| format!("U+{:04X}", *ch as u32))
            .collect();
        format!(" accepted_chars={}", list.join(","))
    } else {
        String::new()
    };
    println!(
        "run-repertoire {} page={} run={} font={} resource={} accepted={} tested={} refused={} subset={} editable={}{}",
        input.display(),
        page,
        // The RESOLVED run text, not what was typed: an empty --find with a
        // --pin-span means the whole operator, and a caller reading this line
        // should not have to guess which run was answered about.
        sanitize_token(&rep.text),
        sanitize_token(&rep.base_font),
        sanitize_token(&rep.resource),
        rep.accepted.len(),
        rep.candidates_tested,
        rep.candidates_tested.saturating_sub(rep.accepted.len()),
        u8::from(rep.embedded_subset),
        u8::from(rep.is_editable()),
        chars,
    );
    exit::SUCCESS
}

/// `font-preflight` — which of a page's font resources `--set-font` would
/// accept for one run (Pass 142.1).
///
/// # Why this is a subcommand and not a flag on `format-text`
///
/// Because it never formats anything. Folding a read-only query into a
/// mutating subcommand means every caller of the query inherits that
/// subcommand's `--output` contract and its refusal exit codes, and a script
/// asking a question would have to explain why it passed no output path.
///
/// # Exit codes
///
/// `OK` when the run was located, **whatever the answers were** — a page on
/// which every font refuses is a successful answer to the question asked, not
/// a failed command. `EDIT_REFUSED` only when the run itself could not be
/// located (no match, unsupported anchor, unresolvable font resource), which
/// is the same contract `format-text` uses for the same failures.
pub(crate) fn cmd_font_preflight(
    input: &Path,
    page: usize,
    find: &str,
    pin_span: Option<&str>,
    candidate: Option<&str>,
    json: bool,
) -> u8 {
    use pdfcer_core::text_edit::{FontAcceptance, Std14Presence};

    if page == 0 {
        eprintln!("pdfcer: --page is 1-based; 0 is not a valid page number");
        return exit::EDIT_REFUSED;
    }
    // Parsed before any file I/O, same as the editing verbs.
    let pin = match pin_span {
        Some(spec) => match parse_pin_span(spec) {
            Ok(span) => Some(span),
            Err(msg) => {
                eprintln!("pdfcer: {msg}");
                return exit::EDIT_REFUSED;
            }
        },
        None => None,
    };
    // Core refuses an unpinned empty find by name; this names the FLAG that
    // would fix it, which core cannot know about.
    if find.is_empty() && pin.is_none() {
        eprintln!(
            "pdfcer: font-preflight needs --find TEXT, or --pin-span START:LEN with an \
             empty --find to mean the whole pinned show operator"
        );
        return exit::EDIT_REFUSED;
    }
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let session = pdfcer_core::edit::EditSession::new(doc);
    let pre = match candidate {
        Some(c) => session.preview_font_resources_for(page - 1, find, pin, c),
        None => session.preview_font_resources(page - 1, find, pin),
    };
    let pre = match pre {
        Ok(p) => p,
        Err(err) => {
            eprintln!("pdfcer: font-preflight refused: {err}");
            return exit::EDIT_REFUSED;
        }
    };
    let std14_rows = |json: bool| -> Vec<String> {
        pre.standard_14
            .iter()
            .map(|e| {
                let (presence, res) = match &e.presence {
                    Std14Presence::OnPage { resource } => ("on-page", Some(resource.clone())),
                    Std14Presence::WouldBeAdded => ("would-add", None),
                    // `Std14Presence` is #[non_exhaustive]; print something.
                    _ => ("other", None),
                };
                let (accepted, refusal, character) = match &e.acceptance {
                    FontAcceptance::Accepted => (true, String::new(), None),
                    FontAcceptance::Refused { message, character } => {
                        (false, message.clone(), *character)
                    }
                    _ => (false, "unknown acceptance".to_owned(), None),
                };
                if json {
                    format!(
                        "    {{ \"base_font\": \"{}\", \"presence\": \"{}\", \"resource\": {}, \"accepted\": {}, \"refusal\": \"{}\", \"refused_character\": {} }}",
                        json_escape(&e.base_font),
                        presence,
                        res.as_deref().map_or_else(|| "null".to_owned(), |r| format!("\"{}\"", json_escape(r))),
                        accepted,
                        json_escape(&refusal),
                        character.map_or_else(|| "null".to_owned(), |c| format!("\"U+{:04X}\"", c as u32)),
                    )
                } else {
                    format!(
                        "  {:<22} {}  {}{}{}",
                        e.base_font,
                        if accepted { "ACCEPT" } else { "REFUSE" },
                        presence,
                        res.as_deref().map_or_else(String::new, |r| format!(" (/{r})")),
                        character.map_or_else(String::new, |c| format!("  refused_character=U+{:04X} '{c}'", c as u32)),
                    )
                }
            })
            .collect()
    };

    if json {
        let mut out = String::from("{\n");
        out.push_str(&format!("  \"text\": \"{}\",\n", json_escape(&pre.text)));
        out.push_str(&format!(
            "  \"run_resource\": \"{}\",\n",
            json_escape(&pre.run_resource)
        ));
        out.push_str(&format!(
            "  \"run_font\": \"{}\",\n",
            json_escape(&pre.run_font)
        ));
        out.push_str(&format!(
            "  \"candidate\": {},\n",
            pre.candidate
                .as_deref()
                .map_or_else(|| "null".to_owned(), |c| format!("\"{}\"", json_escape(c)))
        ));
        out.push_str("  \"standard_14\": [\n");
        out.push_str(&std14_rows(true).join(",\n"));
        out.push_str("\n  ],\n");
        out.push_str("  \"entries\": [\n");
        let rows: Vec<String> = pre
            .entries
            .iter()
            .map(|e| {
                // `FontAcceptance` is `#[non_exhaustive]`, so a downstream
                // crate cannot match it exhaustively — the accessor plus one
                // `if let` is the shape the type is designed for, and it is
                // what any other consumer will have to write too.
                let accepted = e.acceptance.is_accepted();
                let mut refusal = String::new();
                let mut character = None;
                if let FontAcceptance::Refused {
                    message,
                    character: c,
                } = &e.acceptance
                {
                    refusal = message.clone();
                    character = *c;
                }
                let sib = |s: &Option<pdfcer_core::text_edit::FontSibling>| match s {
                    Some(s) => format!(
                        "{{ \"resource\": \"{}\", \"base_font\": \"{}\", \"selector\": \"{}\" }}",
                        json_escape(&s.resource),
                        json_escape(&s.base_font),
                        json_escape(&s.selector)
                    ),
                    None => "null".to_owned(),
                };
                format!(
                    "    {{ \"resource\": \"{}\", \"base_font\": \"{}\", \"selector\": \"{}\", \
                     \"base_font_ambiguous\": {}, \"family\": \"{}\", \"claims_bold\": {}, \
                     \"claims_italic\": {}, \"accepted\": {}, \"refusal\": \"{}\", \
                     \"refused_character\": {}, \"real_bold\": {}, \"real_italic\": {} }}",
                    json_escape(&e.resource),
                    json_escape(&e.base_font),
                    json_escape(&e.selector),
                    e.base_font_ambiguous,
                    json_escape(&e.family),
                    e.claims_bold,
                    e.claims_italic,
                    accepted,
                    json_escape(&refusal),
                    match character {
                        Some(c) => format!("\"U+{:04X}\"", c as u32),
                        None => "null".to_owned(),
                    },
                    sib(&e.real_bold),
                    sib(&e.real_italic)
                )
            })
            .collect();
        out.push_str(&rows.join(",\n"));
        out.push_str("\n  ]\n}\n");
        print!("{out}");
        return exit::SUCCESS;
    }

    // `pre.text` is the RESOLVED text, which on a pinned whole-operator query
    // is the operator's own characters rather than the empty string that was
    // passed in. Printing it is how a caller sees what was actually tested.
    println!(
        "run: /{} {:?} text={:?} candidate={}",
        pre.run_resource,
        pre.run_font,
        pre.text,
        pre.candidate
            .as_deref()
            .map_or_else(|| "none".to_owned(), |c| format!("{c:?}"))
    );
    if pre.candidate.is_some() {
        println!("  (acceptance below is for the CANDIDATE text, not the located text)");
    }
    for e in &pre.entries {
        let verdict = if e.acceptance.is_accepted() {
            "ACCEPT"
        } else {
            "REFUSE"
        };
        let style = match (e.claims_bold, e.claims_italic) {
            (true, true) => " claims=bold,italic",
            (true, false) => " claims=bold",
            (false, true) => " claims=italic",
            (false, false) => "",
        };
        // The ambiguity marker rides on the selector, not on a separate
        // column, because the whole point of the field is "hand THIS to
        // --set-font" and a caller reading only the selector must still be
        // told when it is a resource key standing in for a shared /BaseFont.
        let amb = if e.base_font_ambiguous {
            " (ambiguous /BaseFont — selector is the resource key)"
        } else {
            ""
        };
        println!(
            "  /{}  {}  base_font={:?} selector={:?}{}{} family={} real_bold={} real_italic={}",
            e.resource,
            verdict,
            e.base_font,
            e.selector,
            amb,
            style,
            e.family,
            e.real_bold
                .as_ref()
                .map_or_else(|| "-".to_owned(), |s| format!("/{}", s.resource)),
            e.real_italic
                .as_ref()
                .map_or_else(|| "-".to_owned(), |s| format!("/{}", s.resource)),
        );
        if let FontAcceptance::Refused { message, .. } = &e.acceptance {
            println!("      refusal: {message}");
        }
    }
    let n_ok = pre.accepted().count();
    println!(
        "{} font resource(s) on page {}; {} would be accepted for this run",
        pre.entries.len(),
        page,
        n_ok
    );
    println!(
        "standard-14 (tested for the same text; `would-add` = --set-font authors the resource):"
    );
    for row in std14_rows(false) {
        println!("{row}");
    }
    // Rule 4: the fact that decides a style control is stated outright rather
    // than left for the reader to derive from the per-entry columns.
    match pre.real_bold() {
        Some(s) => println!(
            "bold: a REAL bold face of this run's family is accepted — --set-font {:?}",
            s.selector
        ),
        None => println!(
            "bold: no real bold face of this run's family is a resource ON THIS PAGE. \
             --bold binds the standard-14 bold sibling (Helvetica-Bold, Times-Bold, Courier-Bold; no embedding) \
             when the run's family has one, else synthesises; --set-font names a face outright; \
             --bold-synthetic forces the stroke — see ACCEPT/REFUSE in the standard-14 block above"
        ),
    }
    match pre.real_italic() {
        Some(s) => println!(
            "italic: a REAL italic face of this run's family is accepted — --set-font {:?}",
            s.selector
        ),
        None => println!(
            "italic: no real italic face of this run's family is a resource ON THIS PAGE. \
             --italic binds the standard-14 italic sibling (Helvetica-Oblique, Times-Italic, Courier-Oblique; no embedding) \
             when the run's family has one, else synthesises; --set-font names a face outright; \
             --italic-synthetic forces the stroke — see ACCEPT/REFUSE in the standard-14 block above"
        ),
    }
    exit::SUCCESS
}

/// Escape a string for a JSON string literal (RFC 8259 §7).
///
/// The seven named escapes, plus `\u00XX` for every other C0 control.
/// Non-ASCII characters pass through as UTF-8, which RFC 8259 permits
/// and which keeps extracted CJK and accented text readable in the
/// output instead of turning it into a wall of `\uXXXX`.
///
/// DEL (U+007F) is deliberately **not** escaped: RFC 8259 requires
/// escaping only U+0000–U+001F, and escaping more would be a silent
/// divergence from the format for no benefit.
pub(crate) fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// inspect --forms (Pass 119.0): which content streams a page actually paints
// ---------------------------------------------------------------------------

/// List the form XObjects each requested page paints, with everything a
/// caller needs to aim `edit-text --target form:N` — and, more importantly,
/// everything they need to decide whether they *should*.
///
/// # Why this subcommand exists at all
///
/// A page's visible text is not all in the page's own content stream. On a
/// CAD-exported drawing almost none of it is: the page stream holds the
/// producer's watermark and a form XObject holds every label and the whole
/// title block. Without a listing, an operator whose `edit-text` found nothing
/// has no way to see that the text is in a different stream, and the honest
/// diagnosis ("the pin is pointing at a different buffer") is unactionable
/// without a way to enumerate the buffers.
///
/// # The column that matters most is `paints`
///
/// A form XObject may legally be painted from several pages, and **no clause
/// in either ISO edition binds one to a page** (`FX-N1`). So `paints=6` means
/// editing text inside that form changes six places. The number is computed
/// document-wide and transitively through nesting, which is why this walks the
/// whole document rather than only the requested pages.
///
/// Read-only. Nothing is written, no content stream is mutated.
pub(crate) fn cmd_inspect_forms(input: &Path, pages_spec: &str) -> u8 {
    use pdfcer_core::text_edit::forms;

    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let page_list = match pdfcer_core::page_tree::pages(&doc) {
        Ok(pages) => pages,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    let indices = match parse_pages(pages_spec, page_list.len()) {
        Ok(indices) => indices,
        Err(message) => {
            eprintln!("pdfcer: {}: --pages {message}", input.display());
            return exit::EDIT_REFUSED;
        }
    };

    let view = doc.view();
    // ONE document walk for every page's `paints` column (`forms::invocation_map`).
    let map = forms::invocation_map(&view);

    let mut total = 0u64;
    let mut shared = 0u64;
    let mut incomplete = 0u64;
    println!("inspect --forms {}", input.display());
    for index in indices {
        let Some(page) = page_list.get(index) else {
            continue;
        };
        let scan = forms::scan_page_forms(&view, page);
        println!("  page {}:", index + 1);
        if scan.forms.is_empty() {
            println!("    (none -- every mark on this page comes from its own /Contents)");
        }
        for form in &scan.forms {
            total += 1;
            let set = map.get(&form.id.num);
            let paints = set.map_or(1, forms::InvocationSet::count);
            let at_least = if set.is_some_and(forms::InvocationSet::is_lower_bound) {
                ">="
            } else {
                ""
            };
            if paints > 1 {
                shared += 1;
            }
            // One word, because a form either declares its own resources or
            // inherits the page's -- there is no partial state (see
            // `ResourceTier`, and the tolerance that was built and reverted
            // before it shipped).
            let tier = match form.resource_tier {
                forms::ResourceTier::Own => "own",
                forms::ResourceTier::Page => "inherited-from-page",
                _ => "inherited-from-enclosing-form",
            };
            println!(
                "    /{} object={} depth={} paints={at_least}{paints} pages={} resources={}",
                String::from_utf8_lossy(&form.name),
                form.id.num,
                form.depth,
                set.map_or_else(String::new, |s| s
                    .pages
                    .iter()
                    .map(|p| (p + 1).to_string())
                    .collect::<Vec<_>>()
                    .join(",")),
                tier
            );
        }
        if scan.depth_overflows > 0 || scan.unresolved > 0 || scan.cycles_skipped > 0 {
            incomplete += 1;
            println!(
                "    incomplete: depth_overflows={} unresolved={} cycles_skipped={} -- the counts above are a LOWER BOUND",
                scan.depth_overflows, scan.unresolved, scan.cycles_skipped
            );
        }
    }
    // The stable, locale-invariant summary line, same two-half contract as
    // every other inspect mode.
    println!("forms: total={total} shared={shared} incomplete_pages={incomplete}");
    exit::SUCCESS
}

// ---------------------------------------------------------------------------
// inspect --text-blocks (Pass 14.0): editable text model + block recognition
// ---------------------------------------------------------------------------

/// Recognise and dump a document's editable text-block structure — the
/// READ-ONLY first slice of the Acrobat-style text-editing subsystem
/// (decision 014, Pass 14.0).
///
/// For each requested page it extracts text WITH provenance capture, builds
/// the derived Run→Line→Column→Block model
/// ([`pdfcer_core::text_edit::EditableTextModel`]), and reports the
/// recognised structure with every inference COUNTED — the whole hierarchy
/// is derived (§14.8, S1-S9), so disclosing the counts is the point (rule
/// 4). Nothing is written.
///
/// Output shape mirrors the rest of the CLI's two-half contract: a single
/// stable, locale-invariant summary line, plus a detailed report (text or,
/// with `--json`, a machine document). The summary line's home follows the
/// same rule as `extract-text`: it goes to stdout, except when the JSON
/// payload already occupies stdout, in which case it goes to stderr.
pub(crate) fn cmd_inspect_text_blocks(input: &Path, pages_spec: &str, json: bool) -> u8 {
    use pdfcer_core::text_edit::{BlockRecognitionOptions, EditableTextModel};
    use pdfcer_core::text_extract::{self, ExtractOptions};

    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let page_list = match pdfcer_core::page_tree::pages(&doc) {
        Ok(pages) => pages,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    let indices = match parse_pages(pages_spec, page_list.len()) {
        Ok(indices) => indices,
        Err(message) => {
            eprintln!("pdfcer: {}: --pages {message}", input.display());
            return exit::EDIT_REFUSED;
        }
    };

    // Provenance is captured so the dump can disclose the surgery substrate
    // (operator span, font, size, fill colour) each recognised line rests
    // on. It costs only the pages actually inspected (R20 spirit).
    // The operator's persisted word-gap ratio. Extraction heuristics are
    // the one place where a document that reads fine to a human can
    // extract wrong, so this is a knob worth honouring rather than a
    // constant worth defending.
    let (settings, settings_report) =
        pdfcer_core::settings::Settings::load(pdfcer_core::settings::resolve_store());
    report_settings(&settings_report);
    let options = ExtractOptions::default()
        .with_provenance(true)
        .with_word_gap_ratio(settings.word_gap_ratio)
        // As the `extract-text` path: `TX-A1` and `AT-A1` both change the
        // characters this subcommand reports, so both are the operator's.
        .with_unmappable_code(settings.unmappable_code)
        .with_actual_text(settings.actual_text);
    let recog = BlockRecognitionOptions::default();

    // Accumulators for the stable summary line.
    let mut total_lines = 0u64;
    let mut total_blocks = 0u64;
    let mut total_glyphs = 0u64;
    let mut columns_max = 0usize;
    let mut atomic_runs = 0u64;
    let mut artifact_runs = 0u64;
    let mut multi_column_pages = 0u64;

    let mut report = String::new();
    let mut notes: Vec<String> = Vec::new();

    if json {
        report.push_str("{\n");
        report.push_str(&format!(
            "  \"input\": \"{}\",\n",
            json_escape(&input.display().to_string())
        ));
        report.push_str("  \"pages\": [");
    }

    for (emitted, &index) in indices.iter().enumerate() {
        let page = match text_extract::extract_page(&doc, &page_list[index], index, &options) {
            Ok(page) => page,
            Err(err) => {
                eprintln!("pdfcer: {}: page {}: {err}", input.display(), index + 1);
                return exit::RUNTIME_ERROR;
            }
        };
        let model = EditableTextModel::recognize(&page, &recog);
        let d = model.diagnostics();

        total_lines += d.lines_recognized;
        total_blocks += d.blocks_recognized;
        total_glyphs += d.glyphs_clustered;
        columns_max = columns_max.max(model.columns());
        atomic_runs += d.atomic_runs;
        artifact_runs += d.artifact_runs_skipped;
        if d.is_multi_column() {
            multi_column_pages += 1;
        }
        for note in &d.notes {
            if !notes.contains(note) {
                notes.push(note.clone());
            }
        }

        if json {
            if emitted > 0 {
                report.push(',');
            }
            append_page_json(&mut report, &model);
        } else {
            append_page_report(&mut report, &model);
        }
    }

    if json {
        report.push_str(if indices.is_empty() { "]\n" } else { "\n  ]\n" });
        report.push_str("}\n");
    }

    let summary = format!(
        "text-blocks {}: pages={} lines={} blocks={} columns_max={} glyphs={} \
atomic_runs={} artifact_runs={} multi_column_pages={}",
        input.display(),
        indices.len(),
        total_lines,
        total_blocks,
        columns_max,
        total_glyphs,
        atomic_runs,
        artifact_runs,
        multi_column_pages,
    );

    if json {
        // JSON payload owns stdout; the summary goes to stderr so a caller
        // capturing stdout gets clean JSON.
        print!("{report}");
        eprintln!("{summary}");
    } else {
        // Text mode: the stable summary line first (parseable), then the
        // human report, both on stdout.
        println!("{summary}");
        print!("{report}");
    }

    // The derived-structure disclosures always go to stderr, so they can
    // never be mistaken for sourced content (rule 4).
    for note in &notes {
        eprintln!("pdfcer: {note}");
    }

    exit::SUCCESS
}

/// Append one page's human-readable block report to `out`.
pub(crate) fn append_page_report(
    out: &mut String,
    model: &pdfcer_core::text_edit::EditableTextModel<'_>,
) {
    let page_number = model.sourced_view().page_index + 1;
    let d = model.diagnostics();
    out.push_str(&format!(
        "page {}: columns={} lines={} blocks={} glyphs={} multi_column={}\n",
        page_number,
        model.columns(),
        d.lines_recognized,
        d.blocks_recognized,
        d.glyphs_clustered,
        u8::from(d.is_multi_column()),
    ));
    out.push_str(&format!(
        "  derived: paragraph_breaks_leading={} paragraph_breaks_indent={} \
lines_split_baseline={} atomic_runs={} artifact_runs={}\n",
        d.paragraph_breaks_by_leading,
        d.paragraph_breaks_by_indent,
        d.lines_split_by_baseline,
        d.atomic_runs,
        d.artifact_runs_skipped,
    ));
    for (bi, block) in model.blocks().iter().enumerate() {
        out.push_str(&format!(
            "  block {bi}: column={} kind={} lines={} bbox=[{:.1} {:.1} {:.1} {:.1}]\n",
            block.column,
            block_kind_str(block.kind),
            block.line_indices.len(),
            block.bbox.llx,
            block.bbox.lly,
            block.bbox.urx,
            block.bbox.ury,
        ));
        for &li in &block.line_indices {
            if let Some(line) = model.lines().get(li) {
                out.push_str(&format!("    | {}\n", model.line_text(line)));
            }
        }
    }
}

/// Append one page's block structure to a JSON array being built in `out`.
pub(crate) fn append_page_json(
    out: &mut String,
    model: &pdfcer_core::text_edit::EditableTextModel<'_>,
) {
    let d = model.diagnostics();
    out.push_str("\n    {\n");
    out.push_str(&format!(
        "      \"page\": {},\n",
        model.sourced_view().page_index + 1
    ));
    out.push_str(&format!("      \"columns\": {},\n", model.columns()));
    out.push_str("      \"diagnostics\": {");
    let counters: [(&str, u64); 8] = [
        ("lines_recognized", d.lines_recognized),
        ("columns_recognized", d.columns_recognized),
        ("blocks_recognized", d.blocks_recognized),
        ("glyphs_clustered", d.glyphs_clustered),
        ("paragraph_breaks_by_leading", d.paragraph_breaks_by_leading),
        ("paragraph_breaks_by_indent", d.paragraph_breaks_by_indent),
        ("lines_split_by_baseline", d.lines_split_by_baseline),
        ("atomic_runs", d.atomic_runs),
    ];
    for (i, (name, value)) in counters.iter().enumerate() {
        out.push_str(&format!(
            "{}\"{name}\": {value}",
            if i > 0 { ", " } else { "" }
        ));
    }
    out.push_str(&format!(
        ", \"artifact_runs_skipped\": {}, \"multi_column\": {}}},\n",
        d.artifact_runs_skipped,
        d.is_multi_column(),
    ));
    out.push_str("      \"blocks\": [");
    for (bi, block) in model.blocks().iter().enumerate() {
        if bi > 0 {
            out.push(',');
        }
        out.push_str("\n        {");
        out.push_str(&format!("\"kind\": \"{}\", ", block_kind_str(block.kind)));
        out.push_str(&format!("\"column\": {}, ", block.column));
        out.push_str(&format!(
            "\"bbox\": [{:.2}, {:.2}, {:.2}, {:.2}], ",
            block.bbox.llx, block.bbox.lly, block.bbox.urx, block.bbox.ury
        ));
        out.push_str(&format!(
            "\"text\": \"{}\", ",
            json_escape(&model.block_text(block))
        ));
        out.push_str("\"lines\": [");
        for (i, &li) in block.line_indices.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            if let Some(line) = model.lines().get(li) {
                append_line_json(out, model, line);
            }
        }
        out.push_str("]}");
    }
    out.push_str(if model.blocks().is_empty() {
        "]\n"
    } else {
        "\n      ]\n"
    });
    out.push_str("    }");
}

/// Append one line's JSON, with the representative provenance of its first
/// glyph (the surgery substrate the later Pass builds on).
pub(crate) fn append_line_json(
    out: &mut String,
    model: &pdfcer_core::text_edit::EditableTextModel<'_>,
    line: &pdfcer_core::text_edit::Line,
) {
    out.push('{');
    out.push_str(&format!("\"baseline_y\": {:.2}, ", line.baseline_y));
    out.push_str(&format!("\"size\": {:.2}, ", line.size));
    out.push_str(&format!(
        "\"bbox\": [{:.2}, {:.2}, {:.2}, {:.2}], ",
        line.bbox.llx, line.bbox.lly, line.bbox.urx, line.bbox.ury
    ));
    out.push_str(&format!("\"glyph_count\": {}, ", line.glyphs.len()));
    out.push_str(&format!(
        "\"text\": \"{}\"",
        json_escape(&model.line_text(line))
    ));
    // Representative provenance: the first glyph's, when captured.
    if let Some(&gref) = line.glyphs.first()
        && let Some(prov) = model.provenance(gref)
    {
        out.push_str(", \"provenance\": {");
        out.push_str(&format!(
            "\"content_stream\": \"{}\", ",
            content_stream_str(prov.content_stream)
        ));
        out.push_str(&format!(
            "\"operator_span\": [{}, {}], ",
            prov.operator_span.start,
            prov.operator_span.end()
        ));
        match &prov.font_resource {
            Some(name) => out.push_str(&format!(
                "\"font_resource\": \"{}\", ",
                json_escape(&String::from_utf8_lossy(name))
            )),
            None => out.push_str("\"font_resource\": null, "),
        }
        out.push_str(&format!("\"tf_size\": {:.2}, ", prov.tf_size));
        out.push_str(&format!(
            "\"fill_color\": {}}}",
            fill_color_json(prov.fill_color.as_ref())
        ));
    }
    out.push('}');
}

// ---------------------------------------------------------------------------
// inspect --reflow-preview (Pass 15.0): READ-ONLY within-block reflow preview
// ---------------------------------------------------------------------------

/// Compute and dump a READ-ONLY within-block reflow preview for one block —
/// the FF-A engine's first slice (decision 015, Pass 15.0). Nothing is
/// written: no content-stream mutation, no `EditSession` command, no save
/// (that is Pass 15.1). This subcommand exists to *demonstrate* the derived
/// preview a UI/surgery Pass will consume.
///
/// It extracts the selected page WITH provenance (so words are measured by
/// their real §9.4.4 advances), recognises the block model with
/// **first-line-indent paragraph splitting relaxed** — a right/centre/
/// justified paragraph has ragged left edges that the default indent rule
/// would fragment into single-line blocks, and reflow needs the WHOLE
/// paragraph — then previews the requested block through
/// [`pdfcer_core::text_edit::ReflowEngine`]. The report shows the detected
/// alignment, the greedy re-wrap's new break points and per-line origins,
/// the new block box, and every disclosure (the disclosures go to stderr so
/// they can never be mistaken for sourced content, rule 4).
#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_inspect_reflow_preview(
    input: &Path,
    pages_spec: &str,
    block_index: usize,
    width: Option<f64>,
    align: Option<&str>,
    leading: Option<f64>,
    json: bool,
) -> u8 {
    use pdfcer_core::text_edit::{
        BlockAlignment, EditableTextModel, ReflowEngine, ReflowError, ReflowRequest,
    };
    use pdfcer_core::text_extract::{self, ExtractOptions};

    // Parse the alignment override up front, so a typo fails cleanly before
    // any document work (the R27 fail-clean posture).
    let align_override = match align {
        None => None,
        Some(s) => match BlockAlignment::parse(s) {
            Some(a) => Some(a),
            None => {
                eprintln!(
                    "pdfcer: {}: --align {s}: expected left|right|center|justified",
                    input.display()
                );
                return exit::EDIT_REFUSED;
            }
        },
    };

    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let page_list = match pdfcer_core::page_tree::pages(&doc) {
        Ok(pages) => pages,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    let indices = match parse_pages(pages_spec, page_list.len()) {
        Ok(indices) => indices,
        Err(message) => {
            eprintln!("pdfcer: {}: --pages {message}", input.display());
            return exit::EDIT_REFUSED;
        }
    };
    // Reflow previews exactly one block on one page; take the first page of
    // the selection (a batch pipeline over many blocks scripts the loop).
    let Some(&page_index) = indices.first() else {
        eprintln!("pdfcer: {}: --pages selected no page", input.display());
        return exit::EDIT_REFUSED;
    };
    let Some(page) = page_list.get(page_index) else {
        eprintln!("pdfcer: {}: page out of range", input.display());
        return exit::EDIT_REFUSED;
    };

    let options = ExtractOptions::default().with_provenance(true);
    let extracted = match text_extract::extract_page(&doc, page, page_index, &options) {
        Ok(page) => page,
        Err(err) => {
            eprintln!(
                "pdfcer: {}: page {}: {err}",
                input.display(),
                page_index + 1
            );
            return exit::RUNTIME_ERROR;
        }
    };

    // Relaxed indent recognition via the ONE source of truth in pdfcer-core
    // (Pass 15.2 §0.3): the CLI, the reflow engine/apply path, and the GUI
    // all recognise paragraphs with this identical config.
    let recog = pdfcer_core::text_edit::reflow_recognition_options();
    let model = EditableTextModel::recognize(&extracted, &recog);
    let engine = ReflowEngine::new(&model);

    let req = ReflowRequest::new()
        .with_wrap_width_opt(width)
        .with_alignment_opt(align_override)
        .with_leading_opt(leading)
        .with_page_cropbox(page.crop_box);
    let preview = match engine.preview(block_index, &req) {
        Ok(preview) => preview,
        Err(err) => {
            eprintln!("pdfcer: {}: reflow: {err}", input.display());
            // Every reflow error is an operator error (bad selector / width /
            // an empty block) — refused, not a corrupt-file runtime error.
            // The `_` arm keeps this exhaustive as `ReflowError` grows
            // (it is `#[non_exhaustive]`).
            return match err {
                ReflowError::BlockIndexOutOfRange(..)
                | ReflowError::EmptyBlock(_)
                | ReflowError::BadWidth(_) => exit::EDIT_REFUSED,
                _ => exit::EDIT_REFUSED,
            };
        }
    };

    let summary = format!(
        "reflow-preview {}: page={} block={} align={} align_source={} width={:.1} leading={:.1} \
lines_before={} lines_after={} words={} overflowing_words={} \
new_bbox=[{:.1},{:.1},{:.1},{:.1}] height_delta={:.1} overflow={}",
        input.display(),
        page_index + 1,
        block_index,
        preview.alignment.alignment.as_str(),
        alignment_source_str(preview.alignment.source),
        preview.wrap_width,
        preview.leading,
        preview.lines_before,
        preview.lines_after,
        preview.diagnostics.words,
        preview.diagnostics.overflowing_words,
        preview.new_bbox.llx,
        preview.new_bbox.lly,
        preview.new_bbox.urx,
        preview.new_bbox.ury,
        preview.height_delta(),
        u8::from(preview.overflow.is_some()),
    );

    let report = if json {
        reflow_preview_json(input, page_index, block_index, &preview)
    } else {
        reflow_preview_report(&preview)
    };

    if json {
        print!("{report}");
        eprintln!("{summary}");
    } else {
        println!("{summary}");
        print!("{report}");
    }

    // Every disclosure goes to stderr, never mistakable for sourced content
    // (rule 4).
    for note in &preview.diagnostics.disclosures {
        eprintln!("pdfcer: {note}");
    }

    exit::SUCCESS
}

/// The stable keyword for an [`pdfcer_core::text_edit::AlignmentSource`].
pub(crate) fn alignment_source_str(
    source: pdfcer_core::text_edit::AlignmentSource,
) -> &'static str {
    use pdfcer_core::text_edit::AlignmentSource;
    match source {
        AlignmentSource::Detected => "detected",
        AlignmentSource::SingleLineDefault => "single_line_default",
        AlignmentSource::AmbiguousDefault => "ambiguous_default",
        AlignmentSource::Overridden => "overridden",
        _ => "unknown",
    }
}

/// The human-readable reflow-preview report (text mode).
pub(crate) fn reflow_preview_report(preview: &pdfcer_core::text_edit::ReflowPreview) -> String {
    let a = &preview.alignment;
    let mut out = String::new();
    out.push_str(&format!(
        "detected: alignment={} source={} left_ragged={:.1} right_ragged={:.1} \
mid_ragged={:.1} tol={:.1}\n",
        a.alignment.as_str(),
        alignment_source_str(a.source),
        a.left_ragged_pt,
        a.right_ragged_pt,
        a.mid_ragged_pt,
        a.tolerance_pt,
    ));
    out.push_str(&format!(
        "box: old=[{:.1} {:.1} {:.1} {:.1}] new=[{:.1} {:.1} {:.1} {:.1}] height_delta={:.1}\n",
        preview.old_bbox.llx,
        preview.old_bbox.lly,
        preview.old_bbox.urx,
        preview.old_bbox.ury,
        preview.new_bbox.llx,
        preview.new_bbox.lly,
        preview.new_bbox.urx,
        preview.new_bbox.ury,
        preview.height_delta(),
    ));
    out.push_str(&format!(
        "lines: before={} after={} words={} space_width={:.2}{} leading={:.2}{}\n",
        preview.lines_before,
        preview.lines_after,
        preview.diagnostics.words,
        preview.diagnostics.space_width_pt,
        if preview.diagnostics.space_width_estimated {
            "(est)"
        } else {
            ""
        },
        preview.diagnostics.leading_pt,
        if preview.diagnostics.leading_estimated {
            "(est)"
        } else {
            ""
        },
    ));
    for (i, line) in preview.lines.iter().enumerate() {
        let slack = match line.justified_slack {
            Some(s) => format!("{s:.1}"),
            None => "-".to_string(),
        };
        out.push_str(&format!(
            "  L{i}: words=[{},{}) x={:.1} baseline={:.1} natural={:.1} gaps={} slack={} \
overflow={} | {}\n",
            line.words.start,
            line.words.end,
            line.origin_x,
            line.baseline_y,
            line.natural_width,
            line.gap_count,
            slack,
            u8::from(line.is_overflowing_word),
            line.text,
        ));
    }
    if let Some(ov) = preview.overflow {
        out.push_str(&format!(
            "overflow: past_bottom={:.1} lines_outside={}\n",
            ov.past_bottom_pt, ov.lines_outside
        ));
    }
    out
}

/// The reflow-preview as a JSON document (`--json`).
pub(crate) fn reflow_preview_json(
    input: &Path,
    page_index: usize,
    block_index: usize,
    preview: &pdfcer_core::text_edit::ReflowPreview,
) -> String {
    let a = &preview.alignment;
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str(&format!(
        "  \"input\": \"{}\",\n",
        json_escape(&input.display().to_string())
    ));
    out.push_str(&format!("  \"page\": {},\n", page_index + 1));
    out.push_str(&format!("  \"block\": {block_index},\n"));
    out.push_str("  \"alignment\": {");
    out.push_str(&format!("\"value\": \"{}\", ", a.alignment.as_str()));
    out.push_str(&format!(
        "\"source\": \"{}\", ",
        alignment_source_str(a.source)
    ));
    out.push_str(&format!(
        "\"left_ragged\": {:.2}, \"right_ragged\": {:.2}, \"mid_ragged\": {:.2}, \"tolerance\": {:.2}}},\n",
        a.left_ragged_pt, a.right_ragged_pt, a.mid_ragged_pt, a.tolerance_pt
    ));
    out.push_str(&format!("  \"wrap_width\": {:.2},\n", preview.wrap_width));
    out.push_str(&format!("  \"leading\": {:.2},\n", preview.leading));
    out.push_str(&format!(
        "  \"lines_before\": {}, \"lines_after\": {},\n",
        preview.lines_before, preview.lines_after
    ));
    out.push_str(&format!(
        "  \"old_bbox\": [{:.2}, {:.2}, {:.2}, {:.2}],\n",
        preview.old_bbox.llx, preview.old_bbox.lly, preview.old_bbox.urx, preview.old_bbox.ury
    ));
    out.push_str(&format!(
        "  \"new_bbox\": [{:.2}, {:.2}, {:.2}, {:.2}],\n",
        preview.new_bbox.llx, preview.new_bbox.lly, preview.new_bbox.urx, preview.new_bbox.ury
    ));
    out.push_str(&format!(
        "  \"height_delta\": {:.2},\n",
        preview.height_delta()
    ));
    // Lines.
    out.push_str("  \"lines\": [");
    for (i, line) in preview.lines.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("\n    {");
        out.push_str(&format!(
            "\"words\": [{}, {}], ",
            line.words.start, line.words.end
        ));
        out.push_str(&format!("\"origin_x\": {:.2}, ", line.origin_x));
        out.push_str(&format!("\"baseline_y\": {:.2}, ", line.baseline_y));
        out.push_str(&format!("\"natural_width\": {:.2}, ", line.natural_width));
        out.push_str(&format!("\"gap_count\": {}, ", line.gap_count));
        out.push_str(&format!(
            "\"is_overflowing_word\": {}, ",
            line.is_overflowing_word
        ));
        match line.justified_slack {
            Some(s) => out.push_str(&format!("\"justified_slack\": {s:.2}, ")),
            None => out.push_str("\"justified_slack\": null, "),
        }
        out.push_str(&format!("\"text\": \"{}\"}}", json_escape(&line.text)));
    }
    out.push_str(if preview.lines.is_empty() {
        "],\n"
    } else {
        "\n  ],\n"
    });
    // Overflow.
    match preview.overflow {
        Some(ov) => out.push_str(&format!(
            "  \"overflow\": {{\"past_bottom\": {:.2}, \"lines_outside\": {}}},\n",
            ov.past_bottom_pt, ov.lines_outside
        )),
        None => out.push_str("  \"overflow\": null,\n"),
    }
    // Diagnostics.
    let d = &preview.diagnostics;
    out.push_str("  \"diagnostics\": {");
    out.push_str(&format!("\"words\": {}, ", d.words));
    out.push_str(&format!("\"overflowing_words\": {}, ", d.overflowing_words));
    out.push_str(&format!(
        "\"space_width\": {:.2}, \"space_width_estimated\": {}, ",
        d.space_width_pt, d.space_width_estimated
    ));
    out.push_str(&format!(
        "\"leading\": {:.2}, \"leading_estimated\": {}, ",
        d.leading_pt, d.leading_estimated
    ));
    out.push_str("\"disclosures\": [");
    for (i, note) in d.disclosures.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&format!("\"{}\"", json_escape(note)));
    }
    out.push_str("]}\n");
    out.push_str("}\n");
    out
}

/// A stable identifier for a [`pdfcer_core::text_edit::BlockKind`].
pub(crate) fn block_kind_str(kind: pdfcer_core::text_edit::BlockKind) -> &'static str {
    use pdfcer_core::text_edit::BlockKind;
    match kind {
        BlockKind::Paragraph => "paragraph",
        _ => "other",
    }
}

/// A stable identifier for a [`pdfcer_core::text_extract::ContentStreamRef`].
pub(crate) fn content_stream_str(stream: pdfcer_core::text_extract::ContentStreamRef) -> String {
    use pdfcer_core::text_extract::ContentStreamRef;
    match stream {
        ContentStreamRef::Page => "page".to_string(),
        ContentStreamRef::Form { object } => format!("form:{object}"),
    }
}

/// A JSON value for a fill colour: a tagged string, or `null` for the
/// §8.6.8 default (unset) colour.
pub(crate) fn fill_color_json(color: Option<&pdfcer_core::text_extract::TextColor>) -> String {
    use pdfcer_core::text_extract::TextColor;
    match color {
        None => "null".to_string(),
        Some(TextColor::Gray(g)) => format!("\"gray:{g:.3}\""),
        Some(TextColor::Rgb(r, g, b)) => format!("\"rgb:{r:.3},{g:.3},{b:.3}\""),
        Some(TextColor::Cmyk(c, m, y, k)) => format!("\"cmyk:{c:.3},{m:.3},{y:.3},{k:.3}\""),
        Some(TextColor::Other) => "\"other\"".to_string(),
        Some(_) => "\"unknown\"".to_string(),
    }
}
