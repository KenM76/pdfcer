//! Plan a whole-form recompute — decision 009 posture B's operator-facing
//! act, expressed as a proposal before it is an edit.
//!
//! # A plan, not a mutation
//!
//! [`plan`] reads a document and returns what a recompute **would** change.
//! It writes nothing. Applying the plan is a separate, deliberate call, and
//! that separation is the whole of decision 009 §5.1: merely opening and
//! saving a form must never change a computed `/V`, or pdfcer would be an
//! editor that silently rewrites documents it was asked only to read.
//!
//! It is also project rule 4 — *fuzzy, never sneaky*. A recomputed total is
//! something pdfcer **inferred**: it read a script it did not run and
//! reproduced what it believes the script means. The plan is that inference
//! made visible, field by field, with its inputs, before it becomes document
//! state.
//!
//! # Evaluation order: `/CO` is normative, and pdfcer follows it
//!
//! A calculated field is routinely another calculation's operand — a line
//! total feeding a subtotal feeding a grand total — so the order of
//! evaluation changes the answer.
//!
//! The interactive form dictionary's **`/CO` array (§12.7.2, Table 218)**
//! records that order, and following it is **obligatory**. This is easy to
//! get wrong from the tables alone, and worth stating precisely because the
//! first version of this module got it wrong:
//!
//! - Table 218's own wording is *descriptive* — `/CO` is "An array of
//!   indirect references to field dictionaries with calculation actions,
//!   defining the calculation order in which their values **will be**
//!   recalculated". Read alone, that sounds advisory.
//! - The obligation is in a **different clause**: §12.6.3, Table 196's `C`
//!   row — "The order in which the document's fields are recalculated
//!   **shall** be defined by the `CO` entry in the interactive form
//!   dictionary".
//!
//! So pdfcer evaluates in `/CO` order. That is both the conformant choice and
//! the *parity* choice, and the second is what actually settles it: posture B
//! exists to produce the number a JavaScript-running reader would produce. An
//! order pdfcer preferred on its own reasoning would make pdfcer disagree with
//! every other reader on the same file — which is a worse failure than
//! reproducing an order the document's author arguably got wrong, because
//! only one of the two is *pdfcer's* mistake.
//!
//! # Where the standard stops, and pdfcer has to choose
//!
//! `/CO` is Required whenever any field has a `/AA /C`, so a file that omits
//! it or lists only some of its calculated fields is **non-conforming** — and
//! ISO specifies **no recovery rule**: no default order, no statement that
//! unlisted fields are or are not calculated. The whole normative treatment
//! of form calculation is those two table rows.
//!
//! pdfcer's choices, all disclosed rather than silent:
//!
//! | Situation | What pdfcer does |
//! |---|---|
//! | `/CO` lists every calculated field | Evaluate strictly in `/CO` order. |
//! | `/CO` lists some | `/CO` order first, then the rest in dependency order, counted in [`RecomputePlan::unlisted_calculations`]. |
//! | `/CO` absent entirely | Dependency order throughout, flagged by [`RecomputePlan::order_source`]. |
//!
//! The dependency fallback derives an order from the scripts' own operand
//! references, which is always available and — unlike a trusted order — is
//! **checkable**: a cycle in it is detectable, and detected.
//!
//! Circular dependencies get no normative treatment either. The word
//! *circular* appears six times in the whole standard and never in §12.6 or
//! §12.7, while §14.7.3 carries an explicit "circular chains shall not be
//! used" for role maps — the standard writes anti-circularity rules when it
//! wants one. In the `/CO` path a cycle is harmless, because the order is
//! given and each field is evaluated once; only the derived path has to
//! detect one, and there it skips every field involved
//! ([`Skip::CircularDependency`]).
//!
//! # What a plan never contains
//!
//! A field pdfcer could not compute confidently. Every such field appears in
//! [`RecomputePlan::skipped`] with its reason, never as a change with a
//! guessed value. The plan is an accept-or-reject proposition; there is
//! nothing in it the operator has to know to distrust.

use std::collections::{BTreeMap, BTreeSet};

use crate::forms::{AcroForm, FieldType, parse_acroform};
use crate::object::ObjId;
use crate::view::DocumentView;

use super::calc::{self, CommaPolicy, Computation, Refusal};
use super::disclose::{self, Disclosure};
use super::inventory::{self, FieldScript};
use super::{CalcHelper, ScriptClass};

/// Where the evaluation order came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderSource {
    /// The document's `/CO` array covered every calculated field — the
    /// conforming case, and the one whose results match another reader's.
    CalculationOrder,
    /// `/CO` covered some calculated fields; the rest were ordered by their
    /// dependencies. The file is non-conforming and pdfcer chose an order the
    /// standard does not specify.
    Mixed,
    /// `/CO` was absent or named none of them. The file is non-conforming
    /// (`/CO` is Required once any field carries a `/AA /C`) and the whole
    /// order is pdfcer's own.
    Derived,
    /// There was nothing to order.
    Empty,
}

impl OrderSource {
    /// Whether pdfcer had to invent any part of the order.
    ///
    /// The single bit a disclosure needs: `true` means the results depend on
    /// a choice the standard does not make, so another reader could legitimately
    /// produce different numbers from the same file.
    #[must_use]
    pub const fn is_pdfcer_choice(self) -> bool {
        matches!(self, Self::Mixed | Self::Derived)
    }
}

/// One field a recompute would change.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedChange {
    /// The field's fully-qualified name.
    pub field: String,
    /// The field dictionary's object id — the handle the edit writes through,
    /// and unambiguous where a name is not.
    pub id: ObjId,
    /// The stored value as it is now.
    pub previous: String,
    /// The value pdfcer computed.
    pub proposed: String,
    /// How it was computed, operand by operand.
    pub computation: Computation,
    /// The operator-facing account of the change.
    pub disclosure: Disclosure,
}

/// Why a recognised calculation was left alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skip {
    /// pdfcer declined to compute it — see [`Refusal`].
    Refused(Refusal),
    /// The field participates in a dependency cycle, and there was no `/CO`
    /// order to resolve it.
    ///
    /// `A = SUM(B)` while `B = SUM(A)` has no evaluation order and no
    /// well-defined answer. Reached only on the derived-order path: given a
    /// `/CO`, the order is stated and each field is evaluated once, which is
    /// what another reader does too.
    CircularDependency,
    /// The computed value equals the stored one, so there is nothing to
    /// change.
    ///
    /// Reported rather than dropped: "pdfcer checked this field and it was
    /// already correct" is a different and more useful statement than
    /// silence, particularly on a form where the operator expected a change.
    AlreadyCorrect,
    /// The script sits on a field type that has no value to compute.
    ///
    /// §12.6.3's NOTE 2 says the `K`/`F`/`V`/`C` triggers are "not defined
    /// for button fields", and §12.7.4.2.2 says a pushbutton "shall not use
    /// the `V` and `DV` entries". A calculation on one is outside the model;
    /// writing a `/V` there would create an entry the standard forbids.
    NotAValueField,
}

impl std::fmt::Display for Skip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(r) => write!(f, "{r}"),
            Self::CircularDependency => f.write_str(
                "this field's calculation depends on itself, directly or through \
                 other fields, and the document states no calculation order to \
                 resolve it by",
            ),
            Self::AlreadyCorrect => f.write_str("already holds the computed value"),
            Self::NotAValueField => {
                f.write_str("this is a button field, which has no value for a calculation to write")
            }
        }
    }
}

/// A field a recompute would not change, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct SkippedField {
    /// The field's fully-qualified name.
    pub field: String,
    /// The field dictionary's object id.
    pub id: ObjId,
    /// Why it was skipped.
    pub reason: Skip,
}

/// Everything a recompute would do, before any of it is done.
#[derive(Debug, Clone, PartialEq)]
pub struct RecomputePlan {
    /// The fields whose value would change, in evaluation order.
    ///
    /// Evaluation order, not name order, so an operator reading the plan sees
    /// the cascade in the sequence that produced it — a subtotal after the
    /// line totals it consumes.
    pub changes: Vec<PlannedChange>,
    /// The recognised calculations that would not change, and why.
    pub skipped: Vec<SkippedField>,
    /// Fields whose script pdfcer recognises but cannot reproduce, or does not
    /// recognise at all. Counted, not listed — [`inventory`] lists them, and
    /// duplicating that here would give two places for the same list to drift.
    pub not_reproducible: usize,
    /// Where the evaluation order came from.
    pub order_source: OrderSource,
    /// How many calculated fields the document's `/CO` failed to list.
    ///
    /// Non-zero means the file is non-conforming — `/CO` is Required once any
    /// field carries a `/AA /C` — and that pdfcer ordered those fields by a
    /// rule the standard does not supply.
    pub unlisted_calculations: usize,
}

impl Default for RecomputePlan {
    fn default() -> Self {
        Self {
            changes: Vec::new(),
            skipped: Vec::new(),
            not_reproducible: 0,
            order_source: OrderSource::Empty,
            unlisted_calculations: 0,
        }
    }
}

impl RecomputePlan {
    /// Whether applying the plan would change anything.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Every operand across the plan that was blank or non-numeric and
    /// counted as zero.
    ///
    /// The headline caveat for a whole-form recompute: a non-zero count means
    /// the totals are arithmetically right and computed over a partly-empty
    /// form, which is exactly the situation where a plausible number is most
    /// misleading.
    #[must_use]
    pub fn coerced_operands(&self) -> usize {
        self.changes
            .iter()
            .map(|c| c.computation.coerced_operands())
            .sum()
    }
}

/// Compute what a recompute would change, without changing anything.
///
/// `policy` decides how a comma in a stored value is read — see
/// [`CommaPolicy`], which defaults to refusing to guess.
#[must_use]
pub fn plan(view: &DocumentView<'_>, policy: CommaPolicy) -> RecomputePlan {
    let mut out = RecomputePlan::default();
    let Some(form) = parse_acroform(view) else {
        return out;
    };
    let inv = inventory::inventory(view);
    out.not_reproducible = inv.scripts.iter().filter(|s| !s.is_reproducible()).count();

    let calculations: Vec<&FieldScript> = inv.calculations().collect();
    if calculations.is_empty() {
        return out;
    }

    let ordering = order_calculations(&form, &calculations);
    out.order_source = ordering.source;
    out.unlisted_calculations = ordering.unlisted;

    for index in ordering.cyclic {
        let Some(script) = calculations.get(index) else {
            continue;
        };
        out.skipped.push(SkippedField {
            field: script.field.clone(),
            id: script.id,
            reason: Skip::CircularDependency,
        });
    }

    // Results accumulate here so a downstream calculation sees the value its
    // operand WOULD take, not the stale one in the file. Nothing in this map
    // has been written anywhere.
    let mut overlay: BTreeMap<String, String> = BTreeMap::new();

    for index in ordering.order {
        let Some(script) = calculations.get(index) else {
            continue;
        };
        let ScriptClass::Calculate(CalcHelper::Simple { op, operands }) = &script.class else {
            continue;
        };
        // A button has no value; §12.6.3 NOTE 2 puts these triggers outside
        // the model for one, and §12.7.4.2.2 forbids a pushbutton a `/V`.
        if is_button(&form, script.id) {
            out.skipped.push(SkippedField {
                field: script.field.clone(),
                id: script.id,
                reason: Skip::NotAValueField,
            });
            continue;
        }
        match calc::compute_with_overrides(&form, *op, operands, policy, &overlay) {
            Ok(computation) => {
                let proposed = calc::render_value(computation.value);
                let previous = current_value(&form, &script.field, &overlay);
                if proposed == previous {
                    // Still recorded in the overlay: a downstream field must
                    // read this value whether or not it needed changing.
                    overlay.insert(script.field.clone(), proposed);
                    out.skipped.push(SkippedField {
                        field: script.field.clone(),
                        id: script.id,
                        reason: Skip::AlreadyCorrect,
                    });
                    continue;
                }
                let disclosure =
                    disclose::for_recompute(&script.field, &script.class, &previous, &proposed);
                overlay.insert(script.field.clone(), proposed.clone());
                out.changes.push(PlannedChange {
                    field: script.field.clone(),
                    id: script.id,
                    previous,
                    proposed,
                    computation,
                    disclosure,
                });
            }
            Err(reason) => {
                // A refused field's stored value stays in force for anything
                // downstream — deliberately NOT entered into the overlay, so a
                // dependent calculation reads the file rather than a value
                // pdfcer declined to produce.
                out.skipped.push(SkippedField {
                    field: script.field.clone(),
                    id: script.id,
                    reason: Skip::Refused(reason),
                });
            }
        }
    }
    out
}

/// Whether the field with this id is a button (`/FT /Btn`).
fn is_button(form: &AcroForm, id: ObjId) -> bool {
    form.fields
        .iter()
        .find(|f| f.id == id)
        .is_some_and(|f| f.field_type == Some(FieldType::Button))
}

/// The value a field holds right now, honouring the plan so far.
fn current_value(form: &AcroForm, name: &str, overlay: &BTreeMap<String, String>) -> String {
    overlay.get(name).cloned().unwrap_or_else(|| {
        form.fields
            .iter()
            .find(|f| f.fully_qualified_name == name)
            .map(|f| f.value.display_text())
            .unwrap_or_default()
    })
}

/// A resolved evaluation order, and how much of it pdfcer had to invent.
struct Ordering {
    /// Indices into the calculations slice, in evaluation order.
    order: Vec<usize>,
    /// Indices skipped for participating in a dependency cycle.
    cyclic: Vec<usize>,
    /// Where the order came from.
    source: OrderSource,
    /// How many calculated fields `/CO` did not list.
    unlisted: usize,
}

/// Order the calculations, preferring the document's own `/CO`.
///
/// `/CO`-listed fields come first, in `/CO` order, because that order is
/// normative (§12.6.3 Table 196's `C` row). Anything `/CO` omits — which
/// makes the file non-conforming, with no recovery rule in the standard —
/// follows in dependency order, which is at least derived from the document
/// rather than from array position.
fn order_calculations(form: &AcroForm, calculations: &[&FieldScript]) -> Ordering {
    // `/CO` identifies fields by indirect reference, so ranking is by object
    // id. Ranking by name would be wrong twice over: a name is not unique,
    // and `/CO` never names one.
    let co_rank: BTreeMap<ObjId, usize> = form
        .calc_order
        .iter()
        .enumerate()
        .map(|(i, id)| (*id, i))
        .collect();

    let mut listed: Vec<(usize, usize)> = Vec::new();
    let mut unlisted: Vec<usize> = Vec::new();
    for (i, script) in calculations.iter().enumerate() {
        match co_rank.get(&script.id) {
            Some(rank) => listed.push((*rank, i)),
            None => unlisted.push(i),
        }
    }
    // Stable by rank. A field listed twice in `/CO` — neither permitted nor
    // forbidden by Table 218 — is matched at its first rank and evaluated
    // once; evaluating it twice would produce the same value anyway, since
    // nothing between the two positions could change its operands more than
    // the second pass would already see.
    listed.sort_by_key(|(rank, _)| *rank);

    let source = match (listed.is_empty(), unlisted.is_empty()) {
        (true, true) => OrderSource::Empty,
        (false, true) => OrderSource::CalculationOrder,
        (true, false) => OrderSource::Derived,
        (false, false) => OrderSource::Mixed,
    };

    // Only the unlisted tail needs an order derived, and only it can be
    // reported cyclic: the listed head has a stated order, and a cycle within
    // it is resolved by that order exactly as it would be in any other reader.
    let (derived, cyclic) = dependency_order(calculations, &unlisted);

    let mut order: Vec<usize> = listed.into_iter().map(|(_, i)| i).collect();
    order.extend(derived);

    Ordering {
        order,
        cyclic,
        source,
        unlisted: unlisted.len(),
    }
}

/// Order a subset of calculations so each follows the calculated fields it
/// reads, and report the ones that cannot be ordered at all.
///
/// Returns `(order, cyclic)` as indices into `calculations`.
///
/// # The algorithm, and why this one
///
/// A depth-first topological sort with an explicit on-stack marker. Kahn's
/// algorithm would be shorter, but it reports a cycle only as "these nodes
/// were left over", whereas the DFS knows *which* traversal closed the loop.
/// Since a cycle here becomes an operator-facing message naming the affected
/// fields, knowing them precisely is worth the extra state.
///
/// Only edges **within `subset`** are modelled. An operand that is an
/// ordinary input field imposes no ordering, because its value cannot change
/// during the plan; and an operand already ordered by `/CO` is likewise fixed
/// before this runs.
fn dependency_order(calculations: &[&FieldScript], subset: &[usize]) -> (Vec<usize>, Vec<usize>) {
    if subset.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let members: BTreeSet<usize> = subset.iter().copied().collect();
    // Name → index, restricted to the subset. A duplicate name keeps the
    // first: two calculated fields with one name is a malformed form, and
    // either choice of edge is arbitrary, so the deterministic one wins.
    let mut by_name: BTreeMap<&str, usize> = BTreeMap::new();
    for i in subset {
        if let Some(script) = calculations.get(*i) {
            by_name.entry(script.field.as_str()).or_insert(*i);
        }
    }

    let dependencies = |node: usize| -> Vec<usize> {
        let Some(ScriptClass::Calculate(CalcHelper::Simple { operands, .. })) =
            calculations.get(node).map(|s| &s.class)
        else {
            return Vec::new();
        };
        operands
            .iter()
            .filter_map(|raw| {
                let name = String::from_utf8_lossy(raw);
                by_name.get(name.as_ref()).copied()
            })
            .filter(|d| members.contains(d))
            .collect()
    };

    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        Unvisited,
        OnStack,
        Done,
    }
    let mut mark: BTreeMap<usize, Mark> = subset.iter().map(|i| (*i, Mark::Unvisited)).collect();
    let mut order = Vec::with_capacity(subset.len());
    let mut cyclic = BTreeSet::new();

    // Iterative rather than recursive: a form is untrusted input and a
    // pathological dependency chain must not be able to exhaust the stack.
    for root in subset {
        if mark.get(root).copied() != Some(Mark::Unvisited) {
            continue;
        }
        mark.insert(*root, Mark::OnStack);
        let mut stack = vec![(*root, 0usize, dependencies(*root))];
        while let Some((node, next, deps)) = stack.pop() {
            if let Some(dep) = deps.get(next).copied() {
                stack.push((node, next + 1, deps));
                match mark.get(&dep).copied().unwrap_or(Mark::Done) {
                    Mark::Unvisited => {
                        mark.insert(dep, Mark::OnStack);
                        let dd = dependencies(dep);
                        stack.push((dep, 0, dd));
                    }
                    // A back edge: every field currently on the stack is in
                    // or reaches the cycle. Marking all of them is
                    // deliberately conservative — a field that merely feeds a
                    // cycle has no definite value either.
                    Mark::OnStack => {
                        cyclic.insert(dep);
                        cyclic.extend(stack.iter().map(|(n, _, _)| *n));
                    }
                    Mark::Done => {}
                }
            } else {
                mark.insert(node, Mark::Done);
                order.push(node);
            }
        }
    }
    let clean: Vec<usize> = order.into_iter().filter(|i| !cyclic.contains(i)).collect();
    (clean, cyclic.into_iter().collect())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::pageops::tests_support::build_pdf_bytes;

    /// Whether the fixture's AcroForm carries a `/CO`.
    #[derive(Clone, Copy, PartialEq)]
    enum Co {
        /// `/CO` lists every calculated field, in declaration order — the
        /// conforming shape.
        Listed,
        /// No `/CO` at all — non-conforming, and the case that forces pdfcer
        /// to derive an order.
        Absent,
    }

    /// Build a form: `inputs` are plain text fields, `calcs` are
    /// `(name, op, operands)` calculated fields.
    fn form_doc(inputs: &[(&str, &str)], calcs: &[(&str, &str, &[&str])], co: Co) -> Vec<u8> {
        let mut objects: Vec<(u32, String)> = Vec::new();
        let mut refs: Vec<String> = Vec::new();
        let mut num = 4u32;

        for (name, value) in inputs {
            refs.push(format!("{num} 0 R"));
            objects.push((num, format!("<< /FT /Tx /T ({name}) /V ({value}) >>")));
            num += 1;
        }
        let mut calc_ids: Vec<u32> = Vec::new();
        for (name, op, operands) in calcs {
            let field_num = num;
            let action_num = num + 1;
            num += 2;
            refs.push(format!("{field_num} 0 R"));
            calc_ids.push(field_num);
            objects.push((
                field_num,
                format!("<< /FT /Tx /T ({name}) /V (0) /AA << /C {action_num} 0 R >> >>"),
            ));
            let list = operands
                .iter()
                .map(|o| format!("\"{o}\""))
                .collect::<Vec<_>>()
                .join(",");
            objects.push((
                action_num,
                format!("<< /S /JavaScript /JS (AFSimple_Calculate\\(\"{op}\", [{list}]\\);) >>"),
            ));
        }
        let co_entry = match co {
            Co::Listed => format!(
                " /CO [{}]",
                calc_ids
                    .iter()
                    .map(|n| format!("{n} 0 R"))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            Co::Absent => String::new(),
        };
        objects.insert(
            0,
            (
                1,
                format!(
                    "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [{}]{co_entry} >> >>",
                    refs.join(" ")
                ),
            ),
        );
        objects.insert(
            1,
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned()),
        );
        objects.insert(
            2,
            (
                3,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>".to_owned(),
            ),
        );
        let borrowed: Vec<(u32, &str)> = objects.iter().map(|(n, b)| (*n, b.as_str())).collect();
        build_pdf_bytes(&borrowed)
    }

    fn plan_for(bytes: &[u8]) -> RecomputePlan {
        let doc = Document::from_bytes(bytes.to_vec()).expect("fixture parses");
        let view = DocumentView::new(&doc, doc.bytes(), doc.version());
        plan(&view, CommaPolicy::default())
    }

    /// A single total is planned, with its old and new value both stated.
    #[test]
    fn a_stale_total_is_planned_with_both_values() {
        let bytes = form_doc(
            &[("A", "100"), ("B", "32.5")],
            &[("Total", "SUM", &["A", "B"])],
            Co::Listed,
        );
        let p = plan_for(&bytes);
        assert_eq!(p.changes.len(), 1);
        assert_eq!(p.changes[0].field, "Total");
        assert_eq!(p.changes[0].previous, "0");
        assert_eq!(p.changes[0].proposed, "132.5");
        assert_eq!(p.order_source, OrderSource::CalculationOrder);
        assert!(!p.order_source.is_pdfcer_choice());
        assert!(!p.is_empty());
    }

    /// ★ **`/CO` wins, even when its order is arguably wrong.**
    ///
    /// Grand is listed before Sub, so Grand is computed from Sub's *stale*
    /// stored value — which is exactly what a JavaScript-running reader does
    /// with the same file, because §12.6.3 Table 196's `C` row makes that
    /// order normative.
    ///
    /// pdfcer could compute a "better" number here by ignoring `/CO`. It must
    /// not: posture B exists to reproduce what another reader produces, and a
    /// pdfcer-only answer would make the same document show two different
    /// totals depending on who opened it.
    #[test]
    fn the_documents_stated_order_wins_even_when_it_is_arguably_wrong() {
        let bytes = form_doc(
            &[("A", "10"), ("B", "20"), ("C", "5")],
            &[("Grand", "SUM", &["Sub", "C"]), ("Sub", "SUM", &["A", "B"])],
            Co::Listed,
        );
        let p = plan_for(&bytes);
        let order: Vec<&str> = p.changes.iter().map(|c| c.field.as_str()).collect();
        assert_eq!(
            order,
            vec!["Grand", "Sub"],
            "/CO order, not dependency order"
        );
        let grand = p
            .changes
            .iter()
            .find(|c| c.field == "Grand")
            .expect("planned");
        assert_eq!(
            grand.proposed, "5",
            "Grand read Sub's STALE 0 + C's 5, matching another reader"
        );
        assert_eq!(p.order_source, OrderSource::CalculationOrder);
    }

    /// ★ **With no `/CO`, pdfcer derives a dependency order — and says it
    /// did.**
    ///
    /// The file is non-conforming (`/CO` is Required once any field carries a
    /// `/AA /C`) and the standard supplies no recovery rule, so the order is
    /// pdfcer's own choice. It is a defensible one, and it is disclosed rather
    /// than presented as the document's.
    #[test]
    fn with_no_calc_order_a_dependency_order_is_derived_and_disclosed() {
        let bytes = form_doc(
            &[("A", "10"), ("B", "20"), ("C", "5")],
            &[("Grand", "SUM", &["Sub", "C"]), ("Sub", "SUM", &["A", "B"])],
            Co::Absent,
        );
        let p = plan_for(&bytes);
        let order: Vec<&str> = p.changes.iter().map(|c| c.field.as_str()).collect();
        assert_eq!(order, vec!["Sub", "Grand"], "Sub feeds Grand, so Sub first");
        let grand = p
            .changes
            .iter()
            .find(|c| c.field == "Grand")
            .expect("planned");
        assert_eq!(grand.proposed, "35", "Grand read Sub's fresh 30");
        assert_eq!(p.order_source, OrderSource::Derived);
        assert!(
            p.order_source.is_pdfcer_choice(),
            "and the plan admits the order was pdfcer's"
        );
        assert_eq!(p.unlisted_calculations, 2);
    }

    /// ★ **A dependency cycle with no stated order is detected and every
    /// field in it is skipped by name**, rather than iterated to an arbitrary
    /// fixed point.
    #[test]
    fn a_dependency_cycle_with_no_stated_order_is_detected_and_skipped() {
        let bytes = form_doc(
            &[("A", "1")],
            &[("X", "SUM", &["Y", "A"]), ("Y", "SUM", &["X", "A"])],
            Co::Absent,
        );
        let p = plan_for(&bytes);
        assert!(p.changes.is_empty(), "nothing in a cycle is computed");
        let cyclic: Vec<&str> = p
            .skipped
            .iter()
            .filter(|s| s.reason == Skip::CircularDependency)
            .map(|s| s.field.as_str())
            .collect();
        assert!(cyclic.contains(&"X"), "{cyclic:?}");
        assert!(cyclic.contains(&"Y"), "{cyclic:?}");
        assert!(
            p.skipped[0]
                .reason
                .to_string()
                .contains("depends on itself"),
            "and the reason is explained, not just flagged"
        );
    }

    /// The same cycle WITH a `/CO` is not a cycle at all: the order is
    /// stated, each field is evaluated once, and the result matches what
    /// another reader produces.
    #[test]
    fn the_same_cycle_with_a_stated_order_is_simply_evaluated_once() {
        let bytes = form_doc(
            &[("A", "1")],
            &[("X", "SUM", &["Y", "A"]), ("Y", "SUM", &["X", "A"])],
            Co::Listed,
        );
        let p = plan_for(&bytes);
        assert!(
            !p.skipped
                .iter()
                .any(|s| s.reason == Skip::CircularDependency),
            "a stated order resolves it"
        );
        assert_eq!(p.changes.len(), 2, "both are computed, once each");
        assert_eq!(p.order_source, OrderSource::CalculationOrder);
    }

    /// A field already holding its computed value is reported as checked,
    /// not silently omitted.
    #[test]
    fn a_correct_field_is_reported_as_already_correct() {
        let bytes = form_doc(
            &[("A", "0"), ("B", "0")],
            &[("Total", "SUM", &["A", "B"])],
            Co::Listed,
        );
        let p = plan_for(&bytes);
        assert!(p.is_empty(), "nothing to change");
        assert_eq!(p.skipped.len(), 1);
        assert_eq!(p.skipped[0].reason, Skip::AlreadyCorrect);
    }

    /// A calculation naming a field that does not exist is skipped with the
    /// refusal, and does not stop the rest of the plan.
    #[test]
    fn one_refused_field_does_not_abort_the_rest_of_the_plan() {
        let bytes = form_doc(
            &[("A", "5")],
            &[
                ("Bad", "SUM", &["A", "Nonexistent"]),
                ("Good", "SUM", &["A"]),
            ],
            Co::Listed,
        );
        let p = plan_for(&bytes);
        assert_eq!(p.changes.len(), 1, "the good one is still planned");
        assert_eq!(p.changes[0].field, "Good");
        assert!(matches!(
            p.skipped
                .iter()
                .find(|s| s.field == "Bad")
                .map(|s| &s.reason),
            Some(Skip::Refused(Refusal::UnresolvedOperand(_)))
        ));
    }

    /// A refused field does NOT enter the overlay, so anything downstream
    /// reads the file rather than a value pdfcer declined to produce.
    #[test]
    fn a_refused_field_does_not_feed_the_cascade() {
        let bytes = form_doc(
            &[("A", "5")],
            &[
                ("Bad", "SUM", &["Nonexistent"]),
                ("Uses", "SUM", &["Bad", "A"]),
            ],
            Co::Listed,
        );
        let p = plan_for(&bytes);
        let uses = p
            .changes
            .iter()
            .find(|c| c.field == "Uses")
            .expect("planned");
        assert_eq!(
            uses.proposed, "5",
            "Bad's STORED 0 was used, not a value pdfcer refused to compute"
        );
    }

    /// The plan counts blank operands across the whole form, so a total
    /// computed over a half-filled form can say so.
    #[test]
    fn the_plan_counts_blank_operands_across_the_form() {
        let bytes = form_doc(
            &[("A", "10"), ("B", ""), ("C", "N/A")],
            &[("Total", "SUM", &["A", "B", "C"])],
            Co::Listed,
        );
        let p = plan_for(&bytes);
        assert_eq!(p.changes[0].proposed, "10");
        assert_eq!(p.coerced_operands(), 2);
    }

    /// A form with no scripts, and one with no form at all, both plan
    /// nothing rather than failing.
    #[test]
    fn a_form_with_nothing_to_do_plans_nothing() {
        let bytes = form_doc(&[("A", "1")], &[], Co::Listed);
        let p = plan_for(&bytes);
        assert!(p.is_empty());
        assert_eq!(p.order_source, OrderSource::Empty);

        let bytes = build_pdf_bytes(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (3, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>"),
        ]);
        assert!(plan_for(&bytes).is_empty());
    }

    /// ★ **A calculation on a button field is skipped, not written.**
    ///
    /// §12.6.3 NOTE 2 puts these triggers outside the model for a button, and
    /// §12.7.4.2.2 forbids a pushbutton a `/V` — so writing one would create
    /// an entry the standard says shall not exist.
    #[test]
    fn a_calculation_on_a_button_is_skipped_rather_than_given_a_value() {
        let bytes = build_pdf_bytes(&[
            (
                1,
                "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R 5 0 R] \
                 /CO [5 0 R] >> >>",
            ),
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (3, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>"),
            (4, "<< /FT /Tx /T (A) /V (7) >>"),
            (5, "<< /FT /Btn /Ff 65536 /T (Go) /AA << /C 6 0 R >> >>"),
            (
                6,
                r#"<< /S /JavaScript /JS (AFSimple_Calculate\("SUM", ["A"]\);) >>"#,
            ),
        ]);
        let p = plan_for(&bytes);
        assert!(p.is_empty(), "a button gets no value");
        assert_eq!(
            p.skipped.iter().map(|s| &s.reason).collect::<Vec<_>>(),
            vec![&Skip::NotAValueField]
        );
    }

    /// Every planned change carries a disclosure that names the operands and
    /// says pdfcer did not run the script.
    #[test]
    fn every_planned_change_carries_its_disclosure() {
        let bytes = form_doc(
            &[("A", "1"), ("B", "2")],
            &[("T", "SUM", &["A", "B"])],
            Co::Listed,
        );
        let p = plan_for(&bytes);
        let m = p.changes[0].disclosure.message();
        assert!(m.contains("NOT run"), "{m}");
        assert!(m.contains("the sum of A, B"), "{m}");
        assert!(m.contains("was 0"), "{m}");
    }
}
