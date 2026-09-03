//! # Signature *presence* detection and change-impact classification
//!
//! **This module verifies nothing itself.** It computes no digest, parses no
//! PKCS#7 blob, and validates no certificate chain — integrity and coverage
//! verification is the sibling `signature_verify` module (`Pass 10.1`),
//! re-exported below as `verify`/`verify_all`; trust (chain, revocation,
//! time) is checked by nothing yet. What this file does is answer the one question a
//! structural editor must answer *before* it writes: **what will this
//! save do to the signatures already in this document?**
//!
//! Spec source: `iso32000__s__12.8.md` in the PDF-spec RAG, whose
//! `## VALIDATION MODEL` section was ingested on 2026-07-31 specifically
//! to drive this code. Everything below cites it; nothing below is
//! inferred from training-data recall about "how PDF signatures work",
//! because the audit found that the folk model is wrong in three
//! separate places.
//!
//! ## The two stages, and the naming trap this module exists to avoid
//!
//! §12.8.2.2.2 splits validation in two:
//!
//! | Stage | What it proves | Modality | Applies to |
//! |---|---|---|---|
//! | **1 — byte-range digest** | the bytes `[0,a) ∪ [b,b+c)` are unchanged since signing | `shall` | every signature with a `/ByteRange` |
//! | **2 — permitted-changes analysis** | later revisions stayed inside the author's allowance | `shall` (outcome) | **only** signatures carrying a transform method |
//!
//! §12.8.1 NOTE 1 promises that an incremental update preserves the
//! signed byte range. That is a **stage-1** fact. It says *nothing* about
//! stage 2 — and the RAG states the consequence in as many words:
//!
//! > Reporting stage-1 success as "the signature is still valid" is the
//! > specific error this section exists to prevent.
//!
//! Hence [`SignatureImpact::ByteRangePreserved`] is named for the fact it
//! actually establishes. An earlier draft of this API called that variant
//! `PreservedIncremental`, which is a stage-1 fact wearing a stage-2
//! name; the rename happened before the API shipped, and the old name is
//! recorded here so nobody reintroduces it as a "clearer" alternative.
//! **No front end may render this variant, on its own, as "your
//! signature is still valid."**
//!
//! ## Classifying a signature: `/Reference`, never `/Perms`
//!
//! §12.8.1 makes a signature a **certification** signature iff its
//! `/Reference` array holds a signature-reference dictionary whose
//! `/TransformMethod` is `/DocMDP`. The catalog's `/Perms → /DocMDP`
//! entry **may** also point at it, but is optional — *"It may also be
//! referenced from the DocMDP entry in the permissions dictionary"* — so
//! classifying by `/Perms` alone **misses certification signatures
//! entirely**. [`SignatureCensus`] therefore reads `/Reference` to
//! classify and `/Perms` only to answer a different question (below).
//!
//! ## Detection versus prevention — the distinction that changes pdfce's behaviour
//!
//! Table 258 (§12.8.4), verbatim: *"If this entry is present, consumer
//! applications **shall enforce** the permissions specified by the `P`
//! attribute in the DocMDP transform parameters dictionary."*
//!
//! pdfce is a consumer application. So:
//!
//! - **`/Perms → /DocMDP` present** ⇒ enforcement is a `shall`. pdfce
//!   **refuses** a structural page operation by name
//!   ([`EditError::CertificationForbidsChange`](crate::edit::EditError::CertificationForbidsChange)),
//!   rather than performing it and warning. A warning would be pdfce
//!   declining to do something the spec says it shall do.
//! - **DocMDP in `/Reference` but no `/Perms` entry** ⇒ detection only.
//!   pdfce performs the edit and reports that it invalidates.
//!
//! ## Why every Pass-3.2 operation invalidates at every `P`
//!
//! Table 254's permitted-change lists are **closed** — *"other changes
//! shall invalidate the signature"* is a `shall`, with no minor-change
//! tolerance. Working the seven operations against it:
//!
//! | Operation | P=1 | P=2 | P=3 |
//! |---|---|---|---|
//! | delete / reorder / rotate / arbitrary insert | invalid | invalid | invalid |
//! | *instantiate a `/Templates` page template* | invalid | **permitted** | **permitted** |
//!
//! The second row is the one carve-out, and it is why this module does
//! **not** encode "any page-tree change ⇒ invalid at P=2" as a
//! spec-sourced rule: §12.7.6 template instantiation genuinely grows the
//! page tree and is genuinely permitted at P≥2. pdfce has no
//! template-instantiation feature, so no pdfce operation can currently
//! land in that cell — but the rule is stated as *"none of pdfce's
//! operations are on the permitted list"*, which stays true when the
//! feature is added, rather than as a claim about page trees that would
//! become false.
//!
//! ## ⚠️ The headline NEGATIVE RESULT: plain approval signatures have no stage 2
//!
//! For an approval signature with **no** `/Reference`, ISO 32000-1
//! defines validation in exactly one sentence — *"A signature shall be
//! validated by recomputing the digest and comparing it with the one
//! stored in the signature"* — which is stage 1 and only stage 1. The
//! RAG searched for and confirmed the absence of: any clause saying a
//! post-signing revision invalidates such a signature, any
//! permitted-changes default for approval signatures, and any validation
//! semantics for the `/Changes` array (which is author-asserted, not
//! reader-computed). Its conclusion, verbatim:
//!
//! > So a conforming validator reading ISO 32000-1 alone concludes: an
//! > approval signature over a document that later gained pages, lost
//! > pages, or had pages reordered by incremental update is STILL VALID.
//! > That is almost certainly not what an operator means by "valid".
//!
//! pdfce reports [`SignatureImpact::Invalidated`] for that case anyway,
//! and the reason is a **product decision under rule 4
//! (fuzzy-never-sneaky), not a spec citation** — the RAG separately
//! records that no `shall` governs how a reader reports a verdict, which
//! makes rule 4 the governing authority. The asymmetry that settles it:
//! over-reporting is a reviewable hint an operator can dismiss;
//! under-reporting is pdfce making a silent claim about the integrity of
//! a legal artifact. [`SignatureImpact::documentation_basis`] exposes
//! which of the two footings a given verdict rests on, so a front end
//! can word them differently instead of flattening them.
//!
//! The widely-repeated claim that Acrobat and the PAdES family *do*
//! report such documents as "signed, but altered since signing" is
//! **empirical tool behaviour, explicitly not sourced** in the RAG
//! (`pades__*` is still empty). It is not cited here and must not be
//! cited from here.
//!
//! ## `/FieldMDP` is recognised and deliberately does not change the verdict
//!
//! §12.8.2.4: the FieldMDP transform detects changes to *"the values of a
//! list of form fields"* — scope is form fields only, so a page-tree
//! change **cannot** violate one structurally. It may attach to an
//! approval signature, which refines the negative result above (such a
//! signature does have a stage 2, just a field-scoped one). pdfce records
//! its presence in the census and keeps the conservative verdict, which
//! is the same answer for a different reason — stated rather than left
//! looking like an oversight.
//!
//! ## Naming
//!
//! Nothing here is called `AuthorSignature`, on the RAG's explicit
//! instruction: Table 234's seed-value `MDP /P 0` defines *"an author
//! signature"* to mean an ordinary approval signature, while §12.8.2.2.1
//! uses "the author of a document" to mean **the certifier**. Two
//! incompatible uses of one word in one clause family; the RAG says do
//! not resolve it silently in code, so pdfce uses neither.

use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};

// The verification stage (`Pass 10.1`) lives in its own module; it is
// re-exported here so `signature::verify` is the path a consumer reaches
// for, beside the census and coverage this file already answers.
pub use crate::signature_verify::{Integrity, SignatureVerdict, Trust, verify, verify_all};

/// How many objects a census will look at before giving up.
///
/// pdfce policy (`ARCHITECTURE.md` §10), not spec. The census walks the
/// AcroForm field tree, which is operator-supplied and therefore
/// adversarial input; the walk is already cycle-guarded, and this bounds
/// the pathological-but-acyclic case (a field tree with a million
/// entries) so signature detection cannot become a denial of service on
/// the Save button.
pub const MAX_FIELD_TREE_NODES: usize = 100_000;

/// Maximum depth of the AcroForm field tree walk (pdfce policy).
///
/// §12.7.3.1 makes fields a hierarchy of arbitrary depth in principle;
/// real ones are two or three levels. Anything deeper is damage or
/// hostility, and the same reasoning as
/// [`page_tree::MAX_TREE_DEPTH`](crate::page_tree::MAX_TREE_DEPTH)
/// applies.
pub const MAX_FIELD_TREE_DEPTH: usize = 64;

/// What a save will do to the signatures a document already carries.
///
/// Deliberately three states and not a boolean: "there are no signatures"
/// and "there are signatures and this save keeps their byte range intact"
/// are different facts that deserve different words, and collapsing them
/// is how a front end ends up silent about the second.
///
/// See the module docs for why the middle variant is named for stage 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SignatureImpact {
    /// The document carries no signature dictionary. Nothing to say, and
    /// a front end should add no friction at all.
    None,
    /// Signatures exist, and this save appends rather than rewrites — so
    /// every signature's **byte-range digest (stage 1) still verifies**
    /// (§12.8.1 NOTE 1).
    ///
    /// ⚠️ **This is not "the signature is still valid."** Stage 2 —
    /// whether the changes are ones the signer permitted — is a separate
    /// question this variant makes no claim about. A front end that
    /// renders this as a reassurance is committing precisely the error
    /// §12.8.2.2.2's two-stage split exists to prevent. Pair it with the
    /// uncertainty, or say nothing.
    ByteRangePreserved,
    /// This save invalidates at least one signature.
    ///
    /// Reached three ways, and [`SignatureImpact::documentation_basis`]
    /// distinguishes them: a full rewrite (which disturbs the signed byte
    /// range outright, failing stage 1); a change outside a DocMDP
    /// transform's permitted list (failing stage 2, spec-sourced); or
    /// pdfce's conservative report for a plain approval signature, which
    /// is a product decision rather than a spec citation.
    Invalidated,
}

impl SignatureImpact {
    /// Whether this verdict rests on a **normative clause** or on
    /// pdfce's conservative-reporting policy.
    ///
    /// Exposed because the two deserve different operator-facing wording
    /// and a front end cannot tell them apart from the variant alone. A
    /// spec-sourced invalidation is a statement of fact; a conservative
    /// one is pdfce declining to make a silent claim it cannot support.
    #[must_use]
    pub const fn documentation_basis(self, census: &SignatureCensus) -> ImpactBasis {
        match self {
            Self::None => ImpactBasis::NotApplicable,
            Self::ByteRangePreserved => ImpactBasis::SpecSourced,
            Self::Invalidated => {
                if census.certifications > 0 {
                    ImpactBasis::SpecSourced
                } else {
                    ImpactBasis::ConservativeReport
                }
            }
        }
    }
}

/// On what footing a [`SignatureImpact`] rests. See
/// [`SignatureImpact::documentation_basis`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImpactBasis {
    /// There is no signature, so there is no verdict to justify.
    NotApplicable,
    /// A clause of ISO 32000-1 says so — Table 254's closed
    /// permitted-changes list, or §12.8.1's byte-range coverage.
    SpecSourced,
    /// ISO 32000-1 is **silent**, and pdfce reports the cautious answer
    /// under rule 4 rather than the literal one. See the module docs'
    /// NEGATIVE RESULT.
    ConservativeReport,
}

/// Which save path the impact question is being asked about.
///
/// The answer genuinely differs — an append preserves the signed byte
/// range (§12.8.1 NOTE 1) and a rewrite cannot — so a signature-less
/// query would have to pick one save path and be wrong about the other.
/// That is why this parameter exists even though the Pass 3.2 UI spec
/// sketched the API without it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SaveMode {
    /// §7.5.6 append. Prior bytes untouched.
    Incremental,
    /// One-revision rewrite. Object offsets move, so no signed byte
    /// range survives.
    FullRewrite,
}

/// What signatures a document carries, and of what kinds.
///
/// A count-and-classify report, not a validation result. Every field
/// answers a question some caller genuinely asks; nothing here is
/// decorative, because an unused counter in a security-adjacent type is
/// an invitation to draw a conclusion it does not support.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SignatureCensus {
    /// Signature dictionaries found (§12.8.1 Table 252) — those with a
    /// `/ByteRange`, or a `/Type /Sig`.
    pub signatures: usize,
    /// Of those, how many are **certification** signatures: their
    /// `/Reference` array holds a `SigRef` whose `/TransformMethod` is
    /// `/DocMDP` (§12.8.1). Classified from `/Reference`, never from
    /// `/Perms` — see the module docs.
    pub certifications: usize,
    /// The `/P` access permission of the certification signature, if one
    /// was found (Table 254: 1, 2 or 3).
    ///
    /// **`None` here means "no certification signature", not "no `/P`".**
    /// `/P` is `(Optional)` with **default 2** — absence is *permissive*,
    /// not maximally locked — so a present certification with no `/P`
    /// reports `Some(2)`. Getting that backwards is trap 1 of the three
    /// the RAG names.
    pub certification_permission: Option<u8>,
    /// Whether the catalog's `/Perms` dictionary carries a `/DocMDP`
    /// entry (Table 258).
    ///
    /// This is the **enforcement** switch, and the only field here that
    /// changes what pdfce *does* rather than what it *says*: with it
    /// present, refusing a disallowed change is a `shall`.
    pub perms_enforced: bool,
    /// Signatures carrying a `/FieldMDP` transform (§12.8.2.4).
    ///
    /// Recorded, and deliberately not acted on: FieldMDP's scope is form
    /// field *values*, so a page-tree change cannot violate one
    /// structurally. Present so the census does not look as though it
    /// missed them.
    pub field_mdp: usize,
    /// Whether the AcroForm dictionary's `/SigFlags` has bit 1
    /// (`SignaturesExist`) set — §12.7.2 Table 218.
    ///
    /// A weaker signal than a found signature dictionary: it is a
    /// producer's *assertion*, and a document can carry it with no signed
    /// field (a form prepared for signing but not yet signed). Tracked
    /// separately for exactly that reason, and never counted as a
    /// signature.
    pub sig_flags_declared: bool,
}

impl SignatureCensus {
    /// Whether the document carries at least one signature dictionary.
    ///
    /// Deliberately **not** true for a bare `/SigFlags` declaration: an
    /// unsigned form that merely announces it expects signatures must not
    /// make pdfce warn about destroying something that does not exist.
    #[must_use]
    pub const fn any(&self) -> bool {
        self.signatures > 0
    }

    /// Whether a structural page operation must be **refused** rather
    /// than performed-and-reported.
    ///
    /// True exactly when the catalog's `/Perms → /DocMDP` entry is
    /// present (Table 258: *"consumer applications shall enforce the
    /// permissions"*). At that point declining to enforce would be pdfce
    /// ignoring a `shall`, and no `P` value's permitted list contains any
    /// operation pdfce can currently perform (module docs).
    ///
    /// Note the deliberate asymmetry with [`SignatureCensus::any`]: a
    /// certification signature *without* the `/Perms` entry is detection
    /// only, and pdfce performs the edit and reports the consequence.
    #[must_use]
    pub const fn forbids_structural_change(&self) -> bool {
        self.perms_enforced && self.signatures > 0
    }
}

/// Take a signature census over `graph`.
///
/// Walks three places, because a signature can be reachable from any of
/// them and reading only one is how a detector reports "unsigned" on a
/// signed file:
///
/// 1. the AcroForm field tree (§12.7.3), where a signature lives as a
///    field's `/V` — the ordinary case;
/// 2. the catalog's `/Perms` dictionary (§12.8.4 Table 258), which is
///    where a usage-rights signature (`/UR3`) lives and is **not** a
///    signature field at all;
/// 3. `/SigFlags` (Table 218), recorded as a declaration rather than as
///    a signature.
///
/// Cheap: it resolves dictionaries and reads names, never a stream.
///
/// # Examples
///
/// ```
/// use pdfce_core::document::Document;
/// use pdfce_core::signature::census;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let doc = Document::from_bytes(
///     include_bytes!("../../../fixtures/synthetic/hello.pdf").to_vec(),
/// )?;
/// let found = census(&doc);
/// assert!(!found.any());
/// assert!(!found.forbids_structural_change());
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn census<G: ObjectGraph + ?Sized>(graph: &G) -> SignatureCensus {
    let mut out = SignatureCensus::default();
    let Some(catalog) = graph.catalog_dict() else {
        return out;
    };

    // 1. The AcroForm field tree.
    if let Some(acroform) = catalog
        .get(b"AcroForm")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_dict)
    {
        // Table 218 /SigFlags bit 1 (value 1) = SignaturesExist.
        out.sig_flags_declared = acroform
            .get(b"SigFlags")
            .map(|o| graph.resolve(o))
            .and_then(Object::as_int)
            .is_some_and(|flags| flags & 1 != 0);

        if let Some(fields) = acroform
            .get(b"Fields")
            .map(|o| graph.resolve(o))
            .and_then(Object::as_array)
        {
            let mut visited = Vec::new();
            let mut budget = MAX_FIELD_TREE_NODES;
            walk_fields(graph, fields, 0, &mut visited, &mut budget, &mut out);
        }
    }

    // 2. The permissions dictionary (§12.8.4). `/DocMDP` here is the
    //    enforcement switch; `/UR3` is a usage-rights signature that no
    //    field tree references.
    if let Some(perms) = catalog
        .get(b"Perms")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_dict)
    {
        if let Some(sig) = perms
            .get(b"DocMDP")
            .map(|o| graph.resolve(o))
            .and_then(Object::as_dict)
        {
            out.perms_enforced = true;
            // The same dictionary is usually already counted through the
            // field tree; `classify` is idempotent per call site, so this
            // is guarded to avoid double counting.
            if out.certifications == 0 {
                classify(graph, sig, &mut out);
            }
        }
        if let Some(ur) = perms
            .get(b"UR3")
            .map(|o| graph.resolve(o))
            .and_then(Object::as_dict)
        {
            classify(graph, ur, &mut out);
        }
    }
    out
}

/// Recursive AcroForm field-tree walk (§12.7.3.1: a field's `/Kids` may
/// hold further fields, or the widget annotations that render it).
///
/// Guarded on three axes — depth, total nodes, and a visited set — for
/// the reasons `page_tree`'s walk is: this is untrusted input, and a
/// `/Kids` cycle is trivial to author.
fn walk_fields<G: ObjectGraph + ?Sized>(
    graph: &G,
    fields: &[Object],
    depth: usize,
    visited: &mut Vec<ObjId>,
    budget: &mut usize,
    out: &mut SignatureCensus,
) {
    if depth > MAX_FIELD_TREE_DEPTH {
        return;
    }
    for field in fields {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        if let Some(id) = field.as_reference() {
            if visited.contains(&id) {
                continue;
            }
            visited.push(id);
        }
        let Some(dict) = graph.resolve(field).as_dict() else {
            continue;
        };
        // A signature field holds its signature in `/V` (§12.7.4.5).
        // `/FT` is inheritable down the field tree, so a missing `/FT`
        // here does not mean "not a signature field" — testing the VALUE
        // for signature-dictionary shape is what makes this robust
        // against that, and against a field tree whose `/FT` sits on an
        // ancestor.
        if let Some(value) = dict.get(b"V").map(|o| graph.resolve(o))
            && let Some(sig) = value.as_dict()
            && is_signature_dict(graph, sig)
        {
            classify(graph, sig, out);
        }
        if let Some(kids) = dict
            .get(b"Kids")
            .map(|o| graph.resolve(o))
            .and_then(Object::as_array)
        {
            walk_fields(graph, kids, depth + 1, visited, budget, out);
        }
    }
}

/// Whether a dictionary is a signature dictionary (Table 252).
///
/// Two independent tests, either sufficient:
///
/// - `/Type /Sig` — `(Optional)` in Table 252, so its **absence proves
///   nothing** and it cannot be the only test;
/// - a `/ByteRange` entry — which Table 252 marks *"(Required for all
///   signatures…)"*, making it the reliable structural marker.
fn is_signature_dict<G: ObjectGraph + ?Sized>(graph: &G, dict: &Dict) -> bool {
    let typed = dict
        .get(b"Type")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_name)
        .is_some_and(|n| n.as_bytes() == b"Sig");
    typed || dict.contains_key(b"ByteRange")
}

/// Count one signature dictionary and classify it by its `/Reference`
/// transform methods (§12.8.1).
fn classify<G: ObjectGraph + ?Sized>(graph: &G, sig: &Dict, out: &mut SignatureCensus) {
    out.signatures += 1;

    let Some(references) = sig
        .get(b"Reference")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_array)
    else {
        // No transform method: a plain approval signature. Per the module
        // docs' NEGATIVE RESULT it has no stage 2 in ISO 32000-1 at all.
        return;
    };

    for reference in references {
        let Some(sigref) = graph.resolve(reference).as_dict() else {
            continue;
        };
        let method = sigref
            .get(b"TransformMethod")
            .map(|o| graph.resolve(o))
            .and_then(Object::as_name)
            .map(Name::as_bytes)
            .unwrap_or_default()
            .to_vec();
        match method.as_slice() {
            b"DocMDP" => {
                out.certifications += 1;
                // Table 254: `/P` is (Optional) with DEFAULT 2. Absence
                // is permissive, not strict — trap 1 of three.
                let permission = sigref
                    .get(b"TransformParams")
                    .map(|o| graph.resolve(o))
                    .and_then(Object::as_dict)
                    .and_then(|params| params.get(b"P").map(|o| graph.resolve(o)))
                    .and_then(Object::as_int)
                    .and_then(|p| u8::try_from(p).ok())
                    .filter(|p| (1..=3).contains(p))
                    .unwrap_or(2);
                // The strictest wins if a malformed file somehow carries
                // two (§12.8.2.2.1 allows at most one).
                out.certification_permission = Some(
                    out.certification_permission
                        .map_or(permission, |existing| existing.min(permission)),
                );
            }
            b"FieldMDP" => out.field_mdp += 1,
            // `/UR` (usage rights) and any unknown method: counted as a
            // signature, not classified further. UR is an entitlement
            // mechanism, not a document-integrity one (§12.8.2.3).
            _ => {}
        }
    }
}

/// Classify what `mode` will do to the signatures `census` found.
///
/// The whole decision table, in one place, so a front end never has to
/// reconstruct it:
///
/// | census | mode | verdict | why |
/// |---|---|---|---|
/// | no signatures | either | [`SignatureImpact::None`] | nothing to affect |
/// | any signature | `FullRewrite` | [`SignatureImpact::Invalidated`] | object offsets move, so no signed byte range survives (stage 1 fails) |
/// | any signature | `Incremental`, **no structural change** | [`SignatureImpact::ByteRangePreserved`] | §12.8.1 NOTE 1 — and *only* that |
/// | any signature | `Incremental`, **structural change** | [`SignatureImpact::Invalidated`] | Table 254's closed list for a certification; conservative report otherwise |
///
/// `structural` is the caller's assertion that this save changes the
/// page tree — the parameter exists because *this module cannot see the
/// dirty set*, and inferring "structural" from object counts would be a
/// guess dressed as a fact.
#[must_use]
pub const fn impact_of(
    census: &SignatureCensus,
    mode: SaveMode,
    structural: bool,
) -> SignatureImpact {
    if !census.any() {
        return SignatureImpact::None;
    }
    match mode {
        SaveMode::FullRewrite => SignatureImpact::Invalidated,
        SaveMode::Incremental => {
            if structural {
                SignatureImpact::Invalidated
            } else {
                SignatureImpact::ByteRangePreserved
            }
        }
    }
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

    /// Build a small classic PDF whose catalog carries `catalog_extra`
    /// and whose body carries `extra` objects, numbered from 3.
    fn build(catalog_extra: &str, extra: &[&str]) -> Document {
        let mut bodies = vec![
            format!("<< /Type /Catalog /Pages 2 0 R {catalog_extra}>>"),
            "<< /Type /Pages /Kids [] /Count 0 >>".to_owned(),
        ];
        bodies.extend(extra.iter().map(|s| (*s).to_owned()));

        let mut buf = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (i, body) in bodies.iter().enumerate() {
            offsets.push(buf.len());
            buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
        }
        let xref_at = buf.len();
        let size = bodies.len() + 1;
        buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
        for off in &offsets {
            buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        buf.extend_from_slice(
            format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
                .as_bytes(),
        );
        Document::from_bytes(buf).unwrap()
    }

    #[test]
    fn an_unsigned_document_reports_nothing() {
        let doc = build("", &[]);
        let c = census(&doc);
        assert!(!c.any());
        assert_eq!(c.signatures, 0);
        assert!(!c.forbids_structural_change());
        assert_eq!(
            impact_of(&c, SaveMode::Incremental, true),
            SignatureImpact::None
        );
    }

    #[test]
    fn sig_flags_alone_is_a_declaration_and_not_a_signature() {
        // A form prepared for signing but not yet signed must not make
        // pdfce warn about destroying a signature that does not exist.
        let doc = build("/AcroForm << /Fields [] /SigFlags 3 >> ", &[]);
        let c = census(&doc);
        assert!(c.sig_flags_declared);
        assert!(!c.any(), "/SigFlags is an assertion, not a signature");
        assert_eq!(
            impact_of(&c, SaveMode::Incremental, true),
            SignatureImpact::None
        );
    }

    #[test]
    fn a_plain_approval_signature_is_found_through_the_field_tree() {
        // Table 252 marks /Type (Optional), so detection must work from
        // /ByteRange alone — this fixture deliberately omits /Type.
        let doc = build(
            "/AcroForm << /Fields [3 0 R] /SigFlags 3 >> ",
            &[
                "<< /FT /Sig /T (Signature1) /V 4 0 R >>",
                "<< /ByteRange [0 100 200 300] /Filter /Adobe.PPKLite >>",
            ],
        );
        let c = census(&doc);
        assert_eq!(c.signatures, 1);
        assert_eq!(c.certifications, 0);
        assert!(!c.forbids_structural_change(), "no /Perms ⇒ detection only");
    }

    #[test]
    fn an_approval_signature_invalidation_is_a_conservative_report_not_a_citation() {
        // The module's headline NEGATIVE RESULT: ISO 32000-1 defines NO
        // stage 2 for a plain approval signature, so this verdict is
        // pdfce's rule-4 policy and must be labelled as such.
        let doc = build(
            "/AcroForm << /Fields [3 0 R] >> ",
            &[
                "<< /FT /Sig /T (Signature1) /V 4 0 R >>",
                "<< /ByteRange [0 100 200 300] >>",
            ],
        );
        let c = census(&doc);
        let impact = impact_of(&c, SaveMode::Incremental, true);
        assert_eq!(impact, SignatureImpact::Invalidated);
        assert_eq!(
            impact.documentation_basis(&c),
            ImpactBasis::ConservativeReport
        );
    }

    #[test]
    fn a_certification_signature_is_classified_from_reference_not_perms() {
        // §12.8.1: /Perms → /DocMDP is OPTIONAL, so a detector that keys
        // on it misses this document's certification entirely.
        let doc = build(
            "/AcroForm << /Fields [3 0 R] >> ",
            &[
                "<< /FT /Sig /T (Sig1) /V 4 0 R >>",
                "<< /ByteRange [0 100 200 300] /Reference [5 0 R] >>",
                "<< /Type /SigRef /TransformMethod /DocMDP /TransformParams 6 0 R >>",
                "<< /Type /TransformParams /P 1 /V /1.2 >>",
            ],
        );
        let c = census(&doc);
        assert_eq!(c.certifications, 1);
        assert_eq!(c.certification_permission, Some(1));
        assert!(
            !c.perms_enforced,
            "detection only — /Perms is what would make it prevention"
        );
        assert!(!c.forbids_structural_change());
        let impact = impact_of(&c, SaveMode::Incremental, true);
        assert_eq!(impact, SignatureImpact::Invalidated);
        // Table 254's list is closed, so THIS one is a spec citation.
        assert_eq!(impact.documentation_basis(&c), ImpactBasis::SpecSourced);
    }

    #[test]
    fn a_missing_transform_permission_defaults_to_two_not_one() {
        // Trap 1: /P is (Optional) with DEFAULT 2. Absence is PERMISSIVE.
        // Reading it as "maximally locked" would make pdfce refuse edits
        // the signer allowed.
        let doc = build(
            "/AcroForm << /Fields [3 0 R] >> ",
            &[
                "<< /FT /Sig /V 4 0 R >>",
                "<< /ByteRange [0 1 2 3] /Reference [5 0 R] >>",
                "<< /TransformMethod /DocMDP /TransformParams 6 0 R >>",
                "<< /Type /TransformParams /V /1.2 >>",
            ],
        );
        assert_eq!(census(&doc).certification_permission, Some(2));
    }

    #[test]
    fn perms_docmdp_upgrades_detection_to_prevention() {
        // Table 258: "consumer applications shall enforce the
        // permissions" — for an editor that means refusing the edit.
        let doc = build(
            "/Perms << /DocMDP 4 0 R >> /AcroForm << /Fields [3 0 R] >> ",
            &[
                "<< /FT /Sig /V 4 0 R >>",
                "<< /ByteRange [0 1 2 3] /Reference [5 0 R] >>",
                "<< /TransformMethod /DocMDP /TransformParams 6 0 R >>",
                "<< /P 2 >>",
            ],
        );
        let c = census(&doc);
        assert!(c.forbids_structural_change());
        assert_eq!(c.certifications, 1, "counted once, not twice");
    }

    #[test]
    fn field_mdp_is_recognised_and_does_not_become_a_certification() {
        // §12.8.2.4: FieldMDP's scope is form-field VALUES; it may attach
        // to an approval signature and cannot be violated by a page op.
        let doc = build(
            "/AcroForm << /Fields [3 0 R] >> ",
            &[
                "<< /FT /Sig /V 4 0 R >>",
                "<< /ByteRange [0 1 2 3] /Reference [5 0 R] >>",
                "<< /TransformMethod /FieldMDP /Data 3 0 R >>",
            ],
        );
        let c = census(&doc);
        assert_eq!(c.field_mdp, 1);
        assert_eq!(c.certifications, 0);
        assert!(!c.forbids_structural_change());
    }

    #[test]
    fn a_full_rewrite_invalidates_even_without_a_structural_change() {
        // Stage 1 itself fails: offsets move, so the signed byte range
        // cannot survive. Nothing about permitted changes is involved.
        let doc = build(
            "/AcroForm << /Fields [3 0 R] >> ",
            &["<< /FT /Sig /V 4 0 R >>", "<< /ByteRange [0 1 2 3] >>"],
        );
        let c = census(&doc);
        assert_eq!(
            impact_of(&c, SaveMode::FullRewrite, false),
            SignatureImpact::Invalidated
        );
        // ...whereas an append that changes nothing structural is the
        // one case that gets the stage-1 name.
        assert_eq!(
            impact_of(&c, SaveMode::Incremental, false),
            SignatureImpact::ByteRangePreserved
        );
    }

    #[test]
    fn a_field_tree_cycle_terminates() {
        // Untrusted input: /Kids pointing back at its own field.
        let doc = build(
            "/AcroForm << /Fields [3 0 R] >> ",
            &["<< /FT /Sig /Kids [3 0 R] >>"],
        );
        let c = census(&doc);
        assert_eq!(c.signatures, 0);
    }
}

// ---------------------------------------------------------------------------
// `/ByteRange` coverage — what a signature actually protects
// ---------------------------------------------------------------------------

/// What one signature's `/ByteRange` covers, measured against the file.
///
/// # Why this is worth reporting WITHOUT any cryptography
///
/// Verifying a signature needs PKCS#7, a certificate chain and a trust
/// store. Knowing **what a signature claims to protect** needs only
/// arithmetic — and it answers a question that a green "signature valid"
/// badge does not:
///
/// > *Was anything added to this file that the signature does not cover?*
///
/// A signature can be cryptographically perfect over the first 40 KB of a
/// 900 KB file. Every byte it covers is genuinely unaltered, and the
/// other 860 KB are unprotected. That is the shape this reports.
///
/// # The modality is the load-bearing part
///
/// §12.8.1 says the range **should** be the entire file — a `should`, not
/// a `shall`. *"Other ranges may be used but since they do not check for
/// all changes to the document, their use is not recommended."*
///
/// So a partial-coverage signature is **conforming**, merely
/// under-protecting. Reporting it as malformed would be wrong, and
/// reporting nothing would leave an operator believing a badge that means
/// less than it looks like. It is reported as what it is.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ByteRangeCoverage {
    /// The signature field's fully-qualified name, when it has one.
    pub field_name: Option<String>,
    /// The `/ByteRange` pairs as written: `(offset, length)`.
    pub ranges: Vec<(u64, u64)>,
    /// Total bytes the digest covers.
    pub covered: u64,
    /// The file's length, for comparison.
    pub file_len: u64,
    /// Bytes after the end of the last covered range.
    ///
    /// **This is the number that matters.** A non-zero tail means content
    /// exists beyond everything the signature protects — the shape an
    /// incremental update takes when it appends a revision after a
    /// signature was applied.
    pub uncovered_tail: u64,
    /// Whether the ranges are ordered and non-overlapping.
    ///
    /// Table 252 calls for *"pairs of integers (starting byte offset,
    /// length in bytes)"* describing the **exact** range. Overlapping or
    /// out-of-order pairs are malformed — unlike partial coverage, which
    /// is not.
    pub ranges_well_formed: bool,
    /// Whether the canonical two-pair shape is used.
    ///
    /// §12.8.1: *"Multiple discontiguous byte ranges shall be used to
    /// describe a digest that does not include the signature value."*
    /// Two pairs, straddling `/Contents`, is what every real producer
    /// writes. A single pair means `/Contents` is inside the digest,
    /// which cannot verify.
    pub pair_count: usize,
}

impl ByteRangeCoverage {
    /// Whether this signature covers the file to its end.
    ///
    /// The honest question behind a "signed" badge. `false` does NOT mean
    /// the signature is invalid — it means it protects less than the
    /// whole document.
    #[must_use]
    pub fn covers_to_eof(&self) -> bool {
        self.uncovered_tail == 0
    }
}

/// Measure every signature's `/ByteRange` against the file's real length.
///
/// Reads the document; changes nothing; needs no cryptography and does
/// not attempt any. **It cannot tell you a signature is VALID** — only
/// what it would be valid *over*. Those are different claims and pdfce
/// must not let one stand in for the other.
///
/// `file_len` is the real byte length of the file as loaded. It is a
/// parameter rather than read from the graph because a `/ByteRange` is a
/// claim about BYTES, and the object model cannot check a claim about
/// bytes against itself.
#[must_use]
pub fn byte_range_coverage<G: ObjectGraph + ?Sized>(
    graph: &G,
    file_len: u64,
) -> Vec<ByteRangeCoverage> {
    let mut out = Vec::new();
    // Reached through `parse_acroform`, not a third field walk. `census`
    // has its own private `walk_fields` and the forms module has the
    // public model; adding a third traversal of the same tree is how the
    // three come to disagree about which fields exist (project rule 2).
    let Some(form) = crate::forms::parse_acroform(graph) else {
        return out;
    };
    for field in &form.fields {
        if field.field_type != Some(crate::forms::FieldType::Signature) {
            continue;
        }
        // The signature dictionary is the field's `/V` (Table 232). An
        // unsigned signature FIELD has no `/V` at all, which is not a
        // defect — it is a form waiting to be signed, and reporting it as
        // uncovered would invent a signature that does not exist.
        let Some(dict) = graph
            .resolved(field.id)
            .as_dict()
            .and_then(|d| d.get(b"V"))
            .map(|o| graph.resolve(o))
            .and_then(Object::as_dict)
        else {
            continue;
        };
        let name = Some(field.fully_qualified_name.clone());
        let Some(arr) = dict.get(b"ByteRange").map(|o| graph.resolve(o)) else {
            continue;
        };
        let Some(items) = arr.as_array() else {
            continue;
        };
        // Integers only. A real number here is malformed — an offset is a
        // byte position, and rounding one would silently move the window
        // a digest is computed over.
        let nums: Vec<i64> = items
            .iter()
            .map(|o| graph.resolve(o))
            .filter_map(Object::as_int)
            .collect();
        if nums.len() != items.len() || nums.len() < 2 || !nums.len().is_multiple_of(2) {
            out.push(ByteRangeCoverage {
                field_name: name,
                ranges: Vec::new(),
                covered: 0,
                file_len,
                // Everything is uncovered when the array cannot be read:
                // reporting zero here would understate the risk in the one
                // case where nothing at all is known.
                uncovered_tail: file_len,
                ranges_well_formed: false,
                pair_count: 0,
            });
            continue;
        }

        let mut ranges: Vec<(u64, u64)> = Vec::with_capacity(nums.len() / 2);
        let mut well_formed = true;
        for pair in nums.chunks_exact(2) {
            // A negative offset or length is nonsense rather than a small
            // number; clamping would invent a range the file never
            // declared.
            let (Some(off), Some(len)) = (pair.first().copied(), pair.get(1).copied()) else {
                well_formed = false;
                break;
            };
            if off < 0 || len < 0 {
                well_formed = false;
                break;
            }
            ranges.push((off.unsigned_abs(), len.unsigned_abs()));
        }

        // Ordered and non-overlapping, per Table 252's "exact" range.
        let mut prev_end = 0u64;
        for (off, len) in &ranges {
            if *off < prev_end {
                well_formed = false;
            }
            prev_end = off.saturating_add(*len);
        }

        let covered: u64 = ranges.iter().map(|(_, l)| *l).sum();
        let end = ranges
            .iter()
            .map(|(o, l)| o.saturating_add(*l))
            .max()
            .unwrap_or(0);
        out.push(ByteRangeCoverage {
            field_name: name,
            pair_count: ranges.len(),
            uncovered_tail: file_len.saturating_sub(end),
            ranges,
            covered,
            file_len,
            ranges_well_formed: well_formed,
        });
    }
    out
}
