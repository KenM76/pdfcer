//! The write-side field resolver — what does this name currently NAME?
//!
//! # Purpose
//!
//! `forms.rs` gives pdfcer a **read projection** of an `/AcroForm`: a flat
//! `Vec<Field>` of terminal fields, with the non-terminal grouping nodes
//! resolved away. That is exactly the right shape for *"show me the fields /
//! fill this one / flatten these"*, it is fuzz-tested, and it is correct.
//!
//! It is the wrong shape for **authoring**, and this module is the other
//! half. Field identity is the fully-qualified name (§12.7.3.2); the FQN is
//! *derived from the object graph's shape*, not stored anywhere; therefore
//! every authoring write must resolve the name against the graph **before**
//! deciding what to write. A projection that has already discarded the
//! grouping nodes cannot answer the question, because the grouping nodes are
//! part of the answer.
//!
//! Decision 020 states this as the binding rule (R100):
//!
//! > Field identity is the fully-qualified name; the fully-qualified name is
//! > derived from the object graph, not stored; therefore every authoring
//! > write must resolve the name against the graph *before* deciding what to
//! > write, and must be able to attach a widget to an existing node without
//! > creating a second node.
//!
//! The last clause is the one with teeth. Two top-level fields with the same
//! `/T` have the same FQN and **no disambiguator** — the file cannot say
//! which one anything meant. pdfcer's own reader copes with that only
//! accidentally (the setters filter by FQN and write to *every* match, a
//! defensive measure for malformed third-party input). Producing that shape
//! deliberately would mean pdfcer authoring the very thing its reader treats
//! as damage, and it cannot be undone afterwards: nothing records which of
//! the two the operator meant.
//!
//! # The one entry point
//!
//! [`resolve_field_path`] is the ONLY way an authoring write may learn what a
//! name denotes. One choke point, so the collision branch exists in exactly
//! one place and cannot drift between the verbs that use it.
//!
//! It walks the raw `/AcroForm /Fields` tree — **including** the
//! non-terminal nodes the read projection discards — and returns one of three
//! [`FieldPath`] answers, which the caller turns into one of four outcomes:
//!
//! | [`FieldPath`] | Requested type | Outcome |
//! |---|---|---|
//! | [`FieldPath::Vacant`] | any | **CREATE** — a new terminal, plus any intermediate parents the path needs |
//! | [`FieldPath::Terminal`] | matches | **MERGE** — attach a widget to the existing node ([`FieldShape`] A→B promotion if needed) |
//! | [`FieldPath::Terminal`] | differs | **REFUSE** — [`FormAuthorError::FieldTypeCollision`] |
//! | [`FieldPath::Grouping`] | any | **REFUSE** — [`FormAuthorError::NameIsGroupingNode`] |
//!
//! The fourth row is the one the parity research did not have. If
//! `Address.City` exists then `Address` names a non-terminal container, and a
//! request for a terminal text field called `Address` is neither a same-type
//! merge nor a different-type collision — the existing node **has no type of
//! its own** (Table 220: *"a non-terminal field does not logically have a
//! type of its own"*). Acrobat's UI has no such branch because it never
//! exposes hierarchy authoring; pdfcer does, so pdfcer needs it.
//!
//! # Dotted names are PATHS, always
//!
//! `--name a.b.c` means non-terminal `a`, non-terminal `a.b`, terminal `c`,
//! reusing whichever of those already exist. §12.7.3.2 reserves the period
//! (2Eh) as the path separator, so a partial name `/T` *containing* one has
//! no unambiguous FQN — [`FormAuthorError::PeriodInPartialName`] refuses it,
//! with no escape hatch, because an escape hatch would author exactly the
//! ambiguity the spec exists to avoid.
//!
//! # What this module does NOT do
//!
//! It reads. It allocates no objects, stages no bytes, and writes nothing —
//! `EditSession` owns all of that, because only the session knows about
//! object allocation, undo commands and the staging buffer. Keeping the
//! resolver a pure function of the graph is what lets it be tested against a
//! hand-built graph with no session at all.

use crate::forms::{ButtonKind, FieldType, MAX_FIELD_TREE_DEPTH, MAX_FORM_FIELDS};
use crate::graph::ObjectGraph;
use crate::object::{ObjId, Object};
use std::collections::HashSet;

/// The physical shape of a terminal field, which decides whether attaching
/// another widget needs a **promotion** first.
///
/// §12.5.6.19 permits a field and its sole widget annotation to be merged
/// into ONE dictionary — but Table 220 permits that only while there is
/// exactly one widget. Attaching a second is therefore a **split**, not an
/// append: the annotation keys must move off the field dict onto a new widget
/// object before a `/Kids` array can exist at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldShape {
    /// Shape A — the field dictionary IS its own single widget (merged).
    /// Attaching a second widget requires promotion to [`Self::KidsWidgets`].
    MergedSingleWidget,
    /// Shape B — the field dictionary has `/Kids`, each entry a widget.
    /// Attaching another widget is an append.
    KidsWidgets {
        /// How many widget kids the field currently has.
        n: usize,
    },
}

/// What a fully-qualified name currently denotes in the field tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldPath {
    /// No node bears this name.
    Vacant {
        /// The LOWEST existing ancestor on the path, or `None` when the path
        /// must be created from the `/AcroForm /Fields` root.
        deepest: Option<ObjId>,
        /// The name segments that must be created beneath `deepest`, in
        /// order. The last is the terminal field's own `/T`; any before it
        /// are non-terminal grouping nodes.
        ///
        /// Never empty: a `Vacant` path has at least the terminal to create.
        remaining: Vec<String>,
    },
    /// A TERMINAL field bears this name — it can hold a value and carry
    /// widgets, so a same-type request MERGES into it.
    Terminal {
        /// The field dictionary's object id.
        id: ObjId,
        /// Its resolved `/FT`, or `None` for a malformed field with none.
        ft: Option<FieldType>,
        /// For a `/Btn`, which kind of button — because `/Btn` alone does not
        /// decide type compatibility: a check box and a radio group are both
        /// `/FT /Btn` and merging one into the other is a type collision.
        kind: Option<ButtonKind>,
        /// Whether attaching a widget needs a promotion first.
        shape: FieldShape,
    },
    /// A NON-TERMINAL grouping node bears this name. Table 220 gives it no
    /// type of its own and it cannot become a fillable field.
    Grouping {
        /// The grouping node's object id.
        id: ObjId,
    },
}

/// Why an authoring write cannot proceed.
///
/// Every variant is a REFUSAL the operator can act on, never an internal
/// failure: each names the field, and says what is in the way.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FormAuthorError {
    /// A terminal field of a DIFFERENT type already bears this name.
    ///
    /// Not merged, because §12.7.3.2 makes same-FQN nodes representations of
    /// ONE field and one field has one type. Renaming either is the fix, and
    /// it is the operator's to choose.
    #[error(
        "a {existing} field named `{fqn}` already exists; a {requested} field cannot share its name"
    )]
    FieldTypeCollision {
        /// The fully-qualified name requested.
        fqn: String,
        /// The type already using it.
        existing: &'static str,
        /// The type requested.
        requested: &'static str,
    },
    /// The name belongs to a non-terminal grouping node.
    ///
    /// `Address` cannot become a text field while `Address.City` exists:
    /// `Address` is the container `City` hangs from, and Table 220 gives a
    /// non-terminal no type of its own.
    #[error("`{fqn}` names a group that contains other fields, so it cannot itself become a field")]
    NameIsGroupingNode {
        /// The fully-qualified name requested.
        fqn: String,
    },
    /// The requested path descends **through an existing TERMINAL field**.
    ///
    /// `Text.2` cannot be created while `Text` is a terminal field: giving
    /// `Text` field-kids makes it non-terminal (§12.7.3.1), and a
    /// non-terminal has no type of its own (Table 220) — so `Text`'s own
    /// `/FT`, `/V` and widget stop belonging to any field.
    ///
    /// # ★★★ WHAT THE BYTES ACTUALLY DO, because the obvious description is
    /// wrong and it was written here first
    ///
    /// This paragraph read *"the value is destroyed and the widget is orphaned
    /// on the page"* until it was checked against a file the defect had
    /// actually produced. **Both halves are false**, and the truth is the more
    /// dangerous shape:
    ///
    /// ```text
    /// 1  /Catalog  /AcroForm << /Fields [6 0 R] >>
    /// 3  /Page     /Annots [6 0 R  9 0 R]
    /// 6  /Annot /Widget /FT /Tx /T (Text) /V (K. Mantle) ... /Kids [9 0 R]
    /// 9  /Annot /Widget /FT /Tx /T (2) /Parent 6 0 R /V ()
    /// ```
    ///
    /// **Nothing is orphaned and nothing is deleted.** Object 6 is still the
    /// sole entry in `/Fields`, still in `/Annots`, and **still carries
    /// `/V (K. Mantle)`**. What it acquired is `/Kids` — so it is now
    /// non-terminal, its `/FT` and `/V` are *inheritable defaults for its
    /// kids* rather than its own, and it is simultaneously a painted `/Widget`
    /// and a non-terminal field, which §12.7.3.1 does not contemplate.
    ///
    /// ⇒ **This is a DEMOTION that makes the data unaddressable, not a
    /// deletion.** Every reader projecting the field tree — pdfcer's included —
    /// correctly reports one field named `Text.2`, while the operator's value
    /// sits in the file reachable by no form verb, no FDF export and no
    /// script. *"Silent data loss"* is still the right name; *"destroyed"* and
    /// *"orphaned"* are not, and a shell hunting for this with an
    /// orphaned-widget accessor would find nothing.
    ///
    /// # ★★ THE MIRROR OF [`Self::NameIsGroupingNode`], AND THE DESTRUCTIVE ONE
    ///
    /// That variant guards *"you asked for a terminal and the name is a
    /// group"*, and has since the choke point was written. This guards *"you
    /// asked for a child and the ancestor is a terminal"* — the same collision
    /// from the other side, and **the more damaging direction**: the first
    /// refuses, and until `Pass 174.8` the second silently **converted and
    /// discarded**. Reported by the consuming shell with a four-command
    /// reproduction; `add-text-field` returned success, `changed=4`, and no
    /// disclosure.
    ///
    /// ★ The resolver has always handed this case back correctly —
    /// [`resolve_field_path`]'s own comment says *"the caller will find
    /// `deepest` is a terminal and can refuse or create beneath it as its own
    /// rules require"*. **No caller refused.** A hole documented at the place
    /// that hands it over is still a hole; the note read as a design note
    /// rather than as an obligation, which is exactly how a half-present guard
    /// stays half-present.
    ///
    /// # Not a conversion, and not a conversion-with-a-disclosure
    ///
    /// Refused outright. **Not** because there is nothing to put back — the
    /// object graph above shows the `/V` survives — but because the operator's
    /// document would already be wrong in a way they cannot see, and an
    /// after-the-fact repair verb is a worse answer than the operation
    /// refusing. That is the consuming shell's own reasoning, adopted.
    ///
    /// A *deliberate* promotion — `Text` becoming a group with the original
    /// demoted to `Text.0`, keeping its value — is a different verb with its
    /// own name and its own confirmation, because it **renames an existing
    /// field**, and a field's name is its identity to every script,
    /// calculation order, FDF import and external mapping that refers to it.
    /// That verb is not built and was not asked for.
    ///
    /// # ★★★ THE MESSAGE BELOW IS SHIPPED UI, AND IT SURVIVED THE CORRECTION
    /// THREE LINES ABOVE IT
    ///
    /// [`crate::edit::EditError::FieldAuthoring`] is `#[error(transparent)]`,
    /// so this template **is verbatim what `pdfcer` and the GUI print**.
    /// `Pass 174.9` corrected *"destroy"* in the rustdoc, in the choke-point
    /// comment and in `docs/core-api` — and stopped **three lines short of the
    /// one copy an operator actually reads**, leaving the withdrawn word in
    /// the only place it was ever going to be seen. Found by
    /// `pdfcer-librarian`'s sweep, not by any gate; `missing_docs` does not
    /// read format strings and no test asserts on this text.
    ///
    /// ⇒ `R222`'s sixth instance, and the one that extends its media list:
    /// **when a doc-comment claim is corrected, the `#[error(…)]` template
    /// beside it is a copy of that claim.** So is a test's `expect_err`
    /// message. Both are prose, neither is compiled against anything.
    #[error(
        "cannot create `{fqn}`: `{terminal}` is already a field, and nesting under it would \
         stop it being one — a field with kids has no type, value or widget of its own \
         (\u{a7}12.7.3.1), so `{terminal}`'s value would stay in the file and become \
         unreachable by name"
    )]
    FieldPathCrossesTerminal {
        /// The fully-qualified name requested.
        fqn: String,
        /// The fully-qualified name of the existing terminal field in the way.
        terminal: String,
    },
    /// A rename would land the field on a name something else already holds.
    ///
    /// **Refused rather than merged, and the asymmetry with creation is
    /// deliberate.** A same-type `add-*` MERGES into an existing name, because
    /// §12.7.3.2 makes same-FQN nodes representations of one field and the
    /// caller asked for a field of that name. A rename did not: the operator
    /// named an EXISTING field and a NEW name, and silently fusing it into an
    /// unrelated field would destroy an identity they never offered up. The
    /// two fields' values, flags and widgets would have to be reconciled, and
    /// nothing in the request says how.
    ///
    /// Deleting or renaming the occupant is the fix, and it is the operator's
    /// to choose.
    #[error("cannot rename `{from}` to `{to}`: a field already bears that name")]
    RenameCollision {
        /// The fully-qualified name being renamed away from.
        from: String,
        /// The fully-qualified name it would have taken.
        to: String,
    },
    /// A dotted PATH was supplied where a single partial name was required.
    ///
    /// Distinct from [`Self::PeriodInPartialName`], and the distinction is
    /// the operator's next move. `A..B` is malformed — no reading of it is
    /// valid. `A.B` is a perfectly well-formed two-level path; it is simply
    /// not a **partial** name, which is one segment by §12.7.3.2's
    /// construction. Telling someone who typed `A.B` that it "contains an
    /// empty name segment" describes a defect their input does not have.
    ///
    /// A rename changes what one node contributes to the path. Accepting a
    /// dotted name here would silently re-parent the field, which is a
    /// different operation and one this verb does not offer.
    #[error(
        "`{supplied}` is a path, not a partial name: a rename sets the ONE segment this field contributes, so it cannot contain a period"
    )]
    DottedPartialName {
        /// What was supplied in place of a single segment.
        supplied: String,
    },
    /// A path segment (a partial name `/T`) contains a period.
    ///
    /// §12.7.3.2 reserves the period as the path separator, so a `/T`
    /// containing one has no unambiguous FQN. There is deliberately no escape
    /// hatch — one would author exactly the ambiguity the spec avoids.
    #[error(
        "`{fqn}` contains an empty name segment; a period separates levels and cannot start, end, or double up"
    )]
    PeriodInPartialName {
        /// The fully-qualified name requested.
        fqn: String,
    },
    /// The requested name is empty, or is only separators/whitespace.
    #[error("a field name cannot be empty")]
    EmptyName,
    /// The path is deeper than [`MAX_FIELD_TREE_DEPTH`].
    ///
    /// The same bound the reader's walk uses, applied on the write side so
    /// pdfcer cannot author a tree its own reader would refuse to descend.
    #[error("`{fqn}` is {depth} levels deep; the limit is {max}")]
    PathTooDeep {
        /// The fully-qualified name requested.
        fqn: String,
        /// How many levels it asked for.
        depth: usize,
        /// The limit.
        max: usize,
    },
}

/// Split a fully-qualified name into its path segments (§12.7.3.2).
///
/// The period is the separator and nothing escapes it, so this is a plain
/// split — with the degenerate results refused rather than silently repaired:
/// a leading, trailing or doubled period yields an EMPTY segment, which would
/// be a field whose partial name is the empty string. That is not a name, and
/// accepting it would put an unaddressable node in the tree.
///
/// # Errors
///
/// [`FormAuthorError::EmptyName`] for an empty or whitespace-only name;
/// [`FormAuthorError::PeriodInPartialName`] for any empty segment;
/// [`FormAuthorError::PathTooDeep`] beyond [`MAX_FIELD_TREE_DEPTH`].
pub fn split_field_path(fqn: &str) -> Result<Vec<String>, FormAuthorError> {
    if fqn.trim().is_empty() {
        return Err(FormAuthorError::EmptyName);
    }
    let segments: Vec<String> = fqn.split('.').map(str::to_owned).collect();
    if segments.iter().any(|s| s.trim().is_empty()) {
        return Err(FormAuthorError::PeriodInPartialName {
            fqn: fqn.to_owned(),
        });
    }
    if segments.len() > MAX_FIELD_TREE_DEPTH {
        return Err(FormAuthorError::PathTooDeep {
            fqn: fqn.to_owned(),
            depth: segments.len(),
            max: MAX_FIELD_TREE_DEPTH,
        });
    }
    Ok(segments)
}

/// Resolve a fully-qualified field name against the live object graph.
///
/// **The ONLY entry point through which an authoring write may learn what a
/// name currently denotes** (R100). It walks the raw `/AcroForm /Fields`
/// tree, retaining the non-terminal grouping nodes that the read projection
/// ([`crate::forms::parse_acroform`]) deliberately discards — because
/// §12.7.3.2 derives the FQN from the tree's SHAPE, so only the tree can
/// answer the question.
///
/// # How the walk decides what a node is
///
/// The same rule the reader uses, and deliberately the same helper predicate,
/// so the two views cannot disagree about a given file: a `/Kids` entry with
/// its own `/T`, `/FT` or `/Kids` is a **child field**; a `/T`-less widget
/// kid is one of the parent's **widgets**. A node with child fields is a
/// grouping node for the purposes of *this* answer even if it also has
/// widgets of its own — because what the caller needs to know is whether the
/// name can become a terminal field, and a node that already contains other
/// fields cannot be renamed out from under them.
///
/// # A note on what `Vacant` promises
///
/// `deepest` is the lowest node on the path that EXISTS; `remaining` is
/// everything below it that must be created. Both halves matter: creating
/// `Personal.Address.Zip` when `Personal` exists must hang the new
/// `Address` off the existing `Personal` rather than adding a SECOND
/// top-level `Personal`, which would be the duplicate-identity defect this
/// whole module exists to make unrepresentable.
///
/// # Errors
///
/// Every [`split_field_path`] refusal. The tree walk itself does not fail:
/// an unreadable or cyclic region simply yields `Vacant`, matching the
/// reader's posture that malformed structure is tolerated rather than fatal.
pub fn resolve_field_path<G: ObjectGraph + ?Sized>(
    graph: &G,
    fqn: &str,
) -> Result<FieldPath, FormAuthorError> {
    let segments = split_field_path(fqn)?;

    // The `/AcroForm /Fields` roots. No form, or an unreadable one, means
    // every name is vacant from the root — which is correct: the caller
    // creates the `/AcroForm` along with the field.
    let roots: Vec<ObjId> = graph
        .catalog_dict()
        .and_then(|c| c.get(b"AcroForm").map(|o| graph.resolve(o)))
        .and_then(Object::as_dict)
        .and_then(|af| af.get(b"Fields").map(|o| graph.resolve(o)))
        .and_then(Object::as_array)
        .map(|a| a.iter().filter_map(Object::as_reference).collect())
        .unwrap_or_default();

    // Descend one segment at a time, tracking the deepest node matched.
    let mut level: Vec<ObjId> = roots;
    let mut deepest: Option<ObjId> = None;
    // Cycle guard, matching the reader's: a `/Kids` loop must terminate.
    let mut visited: HashSet<ObjId> = HashSet::new();

    for (i, segment) in segments.iter().enumerate() {
        let Some(found) = level
            .iter()
            .copied()
            .find(|id| partial_name(graph, *id).as_deref() == Some(segment.as_str()))
        else {
            // This segment does not exist, so neither does anything below it.
            return Ok(FieldPath::Vacant {
                deepest,
                remaining: segments.get(i..).unwrap_or_default().to_vec(),
            });
        };
        if !visited.insert(found) {
            // A cycle. Treat the rest of the path as vacant rather than
            // looping — the same tolerate-don't-panic posture the reader has.
            return Ok(FieldPath::Vacant {
                deepest,
                remaining: segments.get(i..).unwrap_or_default().to_vec(),
            });
        }
        deepest = Some(found);

        let last = i + 1 == segments.len();
        let children = child_fields(graph, found);
        if last {
            return Ok(if children.is_empty() {
                FieldPath::Terminal {
                    id: found,
                    ft: resolved_field_type(graph, found),
                    kind: resolved_button_kind(graph, found),
                    shape: shape_of(graph, found),
                }
            } else {
                // It contains other fields, so it is a container — whatever
                // else it may also carry. See the doc comment.
                FieldPath::Grouping { id: found }
            });
        }
        if children.is_empty() {
            // The path wants to descend THROUGH a terminal field. Nothing
            // below it exists, so the rest of the path is vacant — and the
            // caller will find `deepest` is a terminal and can refuse or
            // create beneath it as its own rules require.
            return Ok(FieldPath::Vacant {
                deepest,
                remaining: segments.get(i + 1..).unwrap_or_default().to_vec(),
            });
        }
        level = children;
    }

    // Unreachable in practice: `segments` is non-empty (`split_field_path`
    // refuses an empty name) and every iteration either returns or continues
    // with a deeper level, so the loop always returns from inside. Stated as
    // a value rather than a panic because an unreachable! in a library
    // function is a crash the caller cannot handle.
    Ok(FieldPath::Vacant {
        deepest,
        remaining: Vec::new(),
    })
}

/// A node's own partial name `/T`, decoded as a §7.9.2 text string.
fn partial_name<G: ObjectGraph + ?Sized>(graph: &G, id: ObjId) -> Option<String> {
    let d = graph.resolved(id).as_dict()?;
    let t = graph.resolve(d.get(b"T")?);
    let bytes = match t {
        Object::String(s) => s.clone(),
        _ => return None,
    };
    Some(crate::edit::decode_text_string(&bytes).text)
}

/// How many of a node's `/Kids` are CHILD FIELDS — zero means **terminal**.
///
/// §12.7.3.1 defines a terminal field as one *"that does not have kids that
/// are fields"*, so this **is** the terminal test, not a proxy for it. Exposed
/// to `edit.rs` because [`FormAuthorError::FieldPathCrossesTerminal`]'s guard
/// lives at the authoring choke point, and a second implementation of
/// "terminal" would be a second place for the reader and the writer to
/// disagree about what a field is.
///
/// A count rather than the `Vec`: the guard asks a yes/no question, and
/// returning the ids would invite a caller to walk them and re-derive the
/// answer differently.
pub(crate) fn child_field_count<G: ObjectGraph + ?Sized>(graph: &G, id: ObjId) -> usize {
    child_fields(graph, id).len()
}

/// A node's fully-qualified name — every ancestor's `/T`, joined by `.`.
///
/// §12.7.3.2: *"the fully qualified field name is the partial field name of
/// the field's ancestors, separated by periods, followed by the field's own
/// partial name."* Used only to NAME the field standing in the way of a
/// refusal, which is the whole value of that refusal: *"`Text.2` cannot be
/// created"* is a restatement of the request, while *"`Text` is already a
/// field"* is the fact the operator has to act on.
///
/// # Why it walks `/Parent` rather than being threaded down from the resolver
///
/// Because the guard sits at the writer's choke point, which is reached from
/// four verbs and has only the object id. Threading the matched prefix through
/// every one of them to save a short upward walk would put the same string in
/// four places, which is where they start to disagree.
///
/// Returns `None` for a node with no `/T` at all. The depth guard mirrors
/// every other tree walk in this crate: a `/Parent` cycle in a hostile file
/// must terminate, and a name is a diagnostic, so a truncated one is a far
/// better outcome than a hang.
pub(crate) fn fully_qualified_name<G: ObjectGraph + ?Sized>(
    graph: &G,
    id: ObjId,
) -> Option<String> {
    const MAX_DEPTH: usize = 64;
    let mut parts = vec![partial_name(graph, id)?];
    let mut current = id;
    let mut seen: HashSet<ObjId> = HashSet::new();
    seen.insert(current);
    for _ in 0..MAX_DEPTH {
        let Some(parent) = graph
            .resolved(current)
            .as_dict()
            .and_then(|d| d.get(b"Parent"))
            .and_then(Object::as_reference)
        else {
            break;
        };
        if !seen.insert(parent) {
            break;
        }
        if let Some(name) = partial_name(graph, parent) {
            parts.push(name);
        }
        current = parent;
    }
    parts.reverse();
    Some(parts.join("."))
}

/// The `/Kids` entries that are CHILD FIELDS rather than bare widgets.
///
/// Uses the same `/T`-or-`/FT`-or-`/Kids` test the reader's `kid_is_field`
/// uses. The two must agree: a kid the reader treats as a field and the
/// resolver treats as a widget would be a node the projection lists and the
/// authoring path is willing to overwrite.
fn child_fields<G: ObjectGraph + ?Sized>(graph: &G, id: ObjId) -> Vec<ObjId> {
    let Some(d) = graph.resolved(id).as_dict() else {
        return Vec::new();
    };
    let Some(kids) = d
        .get(b"Kids")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_array)
    else {
        return Vec::new();
    };
    kids.iter()
        .filter_map(Object::as_reference)
        .take(MAX_FORM_FIELDS)
        .filter(|kid| {
            graph.resolved(*kid).as_dict().is_some_and(|kd| {
                kd.contains_key(b"T") || kd.contains_key(b"FT") || kd.contains_key(b"Kids")
            })
        })
        .collect()
}

/// A node's `/FT`, following `/Parent` upward for the inherited case.
///
/// §12.7.3.1 makes `/FT` inheritable, and a terminal that inherits its type
/// is completely ordinary — the nested fixture's `Zip` and `City` both do. A
/// resolver that only read the node's own `/FT` would report `None` for them
/// and every merge into such a field would look like a malformed-field case.
fn resolved_field_type<G: ObjectGraph + ?Sized>(graph: &G, id: ObjId) -> Option<FieldType> {
    let mut current = Some(id);
    for _ in 0..MAX_FIELD_TREE_DEPTH {
        let d = graph.resolved(current?).as_dict()?;
        if let Some(ft) = d
            .get(b"FT")
            .map(|o| graph.resolve(o))
            .and_then(Object::as_name)
            .and_then(|n| FieldType::from_name(n.as_bytes()))
        {
            return Some(ft);
        }
        current = d.get(b"Parent").and_then(Object::as_reference);
    }
    None
}

/// A `/Btn` node's button kind, from the resolved `/Ff` bits 16/17.
///
/// `None` for anything that is not a button. Returned separately from the
/// type because `/FT /Btn` alone does not decide merge compatibility: a check
/// box and a radio group share it, and merging one into the other would
/// produce a field whose widgets disagree about what they are.
fn resolved_button_kind<G: ObjectGraph + ?Sized>(graph: &G, id: ObjId) -> Option<ButtonKind> {
    if resolved_field_type(graph, id) != Some(FieldType::Button) {
        return None;
    }
    let mut current = Some(id);
    for _ in 0..MAX_FIELD_TREE_DEPTH {
        let d = graph.resolved(current?).as_dict()?;
        if let Some(ff) = d
            .get(b"Ff")
            .map(|o| graph.resolve(o))
            .and_then(Object::as_int)
            .and_then(|v| u32::try_from(v).ok())
        {
            return Some(ButtonKind::from_flags(crate::forms::FieldFlags(ff)));
        }
        current = d.get(b"Parent").and_then(Object::as_reference);
    }
    Some(ButtonKind::from_flags(crate::forms::FieldFlags(0)))
}

/// Whether a terminal field is merged (Shape A) or has widget `/Kids`
/// (Shape B), and how many widgets it currently has.
fn shape_of<G: ObjectGraph + ?Sized>(graph: &G, id: ObjId) -> FieldShape {
    let n = graph
        .resolved(id)
        .as_dict()
        .and_then(|d| d.get(b"Kids").map(|o| graph.resolve(o)))
        .and_then(Object::as_array)
        .map_or(0, |a| a.iter().filter_map(Object::as_reference).count());
    if n == 0 {
        FieldShape::MergedSingleWidget
    } else {
        FieldShape::KidsWidgets { n }
    }
}

/// The annotation keys that move from a merged field dictionary onto the
/// widget object when a Shape A field is promoted to Shape B (§3.1.5 step 1).
///
/// These are the §12.5.2 Table 164 annotation entries plus the widget-specific
/// §12.5.6.19 ones. Everything NOT in this list is a FIELD key (`/FT`, `/T`,
/// `/TU`, `/TM`, `/Ff`, `/V`, `/DV`, `/AA`, `/Opt`, `/MaxLen`, `/Q`) and stays
/// on the field dictionary, because the field is what owns a value and a name.
///
/// `/DA` is deliberately ABSENT. It is legal on both — as a widget's own
/// appearance string and as a field-level default — and pdfcer cannot tell
/// which one a given document meant. Leaving it on the field is the choice
/// that preserves behaviour: a field-level `/DA` is inherited by every widget,
/// so the promoted field's two widgets draw alike, whereas moving it would
/// give the new widget no `/DA` and a different appearance from its sibling.
///
/// `/Type` is absent for the same class of reason: the field dict keeps
/// `/Type /Annot` off, and the new widget gets a fresh `/Type /Annot` written
/// rather than moved, so a document that omitted it does not have the omission
/// propagated.
/// The keys that belong to a FIELD and must never appear on a widget kid.
///
/// The inverse of [`WIDGET_KEYS_TO_MOVE`], and needed for a different reason.
/// That list answers *"what moves off the field during a promotion?"*; this
/// one answers *"what must be stripped from a dictionary before it is used as
/// a widget?"* — because the merge path is handed a dict that was built to be
/// a MERGED field+widget (§12.5.6.19) and is now going to be only the widget
/// half.
///
/// `/T`, `/FT` and `/Kids` are the load-bearing three: the reader classifies a
/// `/Kids` entry as a child FIELD when it carries any of them (R101), so a
/// widget written with a `/T` is not a second view of the field — it is a
/// second field underneath it, silently, composing the FQN `Ref.Ref`.
///
/// The rest are keys that would be harmless to a viewer and wrong to an
/// operator. `/Opt` is the case that surfaced this list: a choice field's
/// option list belongs to the FIELD, and a copy on each widget means a second
/// `add-choice-field` under the same name would leave two disagreeing option
/// lists in one document with no rule for which wins.
///
/// `/DA` is deliberately ABSENT — it is legal on a widget as its own
/// appearance string, and see [`WIDGET_KEYS_TO_MOVE`] for why the promotion
/// leaves it on the field.
pub const FIELD_ONLY_KEYS: &[&[u8]] = &[
    b"T", b"FT", b"Kids", b"Ff", b"V", b"DV", b"AA", b"Opt", b"MaxLen", b"Q", b"TI", b"I", b"RV",
    b"DS", b"TM",
];

pub const WIDGET_KEYS_TO_MOVE: &[&[u8]] = &[
    b"Subtype",
    b"Rect",
    b"AP",
    b"AS",
    b"MK",
    b"F",
    b"BS",
    b"Border",
    b"P",
    b"OC",
    b"StructParent",
    b"H",
    b"A",
    b"C",
    b"CA",
    b"NM",
    b"M",
];

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn a_dotted_name_splits_into_path_segments() {
        assert_eq!(
            split_field_path("Personal.Address.Zip").unwrap(),
            vec!["Personal", "Address", "Zip"],
        );
        assert_eq!(split_field_path("Flat").unwrap(), vec!["Flat"]);
    }

    /// Every degenerate period placement is refused, not repaired.
    ///
    /// A leading, trailing or doubled period yields an EMPTY segment — a
    /// field whose partial name is the empty string. Silently dropping it
    /// would change what the operator asked for; §12.7.3.2 gives the period
    /// one meaning and pdfcer takes it at its word.
    #[test]
    fn an_empty_path_segment_is_refused_however_it_arises() {
        for bad in [".Leading", "Trailing.", "Doubled..Up", "."] {
            assert!(
                matches!(
                    split_field_path(bad),
                    Err(FormAuthorError::PeriodInPartialName { .. }),
                ),
                "{bad} should be refused",
            );
        }
    }

    #[test]
    fn an_empty_name_is_refused() {
        assert_eq!(split_field_path(""), Err(FormAuthorError::EmptyName));
        assert_eq!(split_field_path("   "), Err(FormAuthorError::EmptyName));
    }

    /// The depth bound matches the reader's, so pdfcer cannot author a tree
    /// its own walk would refuse to descend.
    #[test]
    fn a_path_deeper_than_the_readers_limit_is_refused() {
        let deep = vec!["a"; MAX_FIELD_TREE_DEPTH + 1].join(".");
        assert!(matches!(
            split_field_path(&deep),
            Err(FormAuthorError::PathTooDeep { .. }),
        ));
        let ok = vec!["a"; MAX_FIELD_TREE_DEPTH].join(".");
        assert!(split_field_path(&ok).is_ok());
    }
}
