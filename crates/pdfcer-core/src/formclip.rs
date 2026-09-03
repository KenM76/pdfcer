//! **Copy a form field, carry it, plant it somewhere else** — the field
//! clipboard (`FieldClip`), its serialised form, and the two paste policies
//! the operator's two keyboard chords map onto.
//!
//! # Why this module exists rather than six property setters
//!
//! `pdfcer-core` could have grown a `/DA` writer, a `/Q` writer, a `/DV`
//! writer, an `/AA` copier and `/MK` colours on
//! [`BorderSpec`](crate::edit::BorderSpec). That was the alternative the
//! consuming shell weighed and rejected, and the reasoning is worth keeping
//! here because it is the load-bearing design argument for the whole module:
//!
//! **a setter has to EXPRESS a property; a clip only has to CARRY it.**
//!
//! Expressing `/DA` means reconciling a named font against the destination's
//! `/AcroForm /DR /Font` (§12.7.3.3) — a real design problem with its own
//! validity rules. Expressing `/AA` means modelling an action dictionary
//! pdfcer deliberately does not execute (NF4, decision 008 §5.1). Expressing
//! `/MK` means modelling ten appearance-characteristic keys `forms::Widget`
//! declines to model (R43). A clip sidesteps every one of them: it takes the
//! bytes that are already there, owns them, and puts them back.
//!
//! The cost is the one this module pays in full below — a clip must own a
//! **closure**, not a pointer, because the destination is very often a
//! different document.
//!
//! # What a clip is
//!
//! A [`FieldClip`] is *everything a terminal field IS, minus its identity*:
//!
//! | half | keys | why |
//! |---|---|---|
//! | field | `/FT` `/Ff` `/V` `/DV` `/AA` `/Opt` `/MaxLen` `/Q` `/TI` `/I` `/RV` `/DS` `/TM` `/TU` `/DA` `/Lock` `/SV` | Table 220/222 — what a *field* owns |
//! | widget | `/Rect` `/AP` `/AS` `/MK` `/F` `/BS` `/Border` `/OC` `/H` `/A` `/C` `/CA` | Table 164/166/189 — what a *widget* owns |
//! | closure | every object those two reach | so the clip survives leaving the document |
//! | `/DR` font | the `/Font` entry the field's `/DA` names | so `/DA` still resolves in the destination |
//!
//! **`/T` is never carried.** It is the field's identity, and the identity is
//! the operator's decision, supplied through [`FieldPastePolicy`]. So are
//! `/Parent` and `/Kids`: they describe where the field sat in *its* tree.
//!
//! Four widget keys are dropped at **copy** time rather than at paste time,
//! deliberately, so a serialised clip carries nothing that can dangle:
//!
//! - **`/P`** — names the source page. A page reference in another document is
//!   not merely wrong, it resolves to something unrelated.
//! - **`/Parent`** — rewritten by the paste to the field it lands under.
//! - **`/StructParent`** — an index into the *source's* `/StructTreeRoot`
//!   `/ParentTree` (§14.7.4.4). pdfcer has no structure-tree writer, so
//!   carrying the number would point the destination's tag tree at an
//!   arbitrary element. Disclosed on paste into a tagged document.
//! - **`/NM`** — §12.5.2 requires an annotation name to be unique *within its
//!   page*. Carrying one guarantees the opposite the moment a clip is pasted
//!   twice onto one page.
//! - **`/M`** — the source annotation's modification date. A pasted widget is
//!   a **new** annotation; carrying somebody else's timestamp is a claim
//!   about an object that did not exist when it was made. pdfcer never reads a
//!   clock (R80's discipline), so it cannot write a true one either — it
//!   writes none, and says so.
//!
//! # The two chords, and why they are one verb
//!
//! The operator ruled (2026-08-29): *"ctrl v for paste as new. ctrl shift v
//! for paste as duplicate."* Those are [`FieldPastePolicy::NewField`] and
//! [`FieldPastePolicy::AdditionalWidget`], and they are two policies on one
//! verb rather than two verbs because **everything before the branch is
//! identical** — the same encryption, certification, XFA, rectangle, page and
//! accessibility-name guards, run through the same
//! `field_authoring_preflight` all five `add_*_field` verbs already share.
//! Two verbs would be two copies of that sequence, which is exactly the drift
//! that preflight was factored out to prevent.
//!
//! They differ in **which resolver answer they accept**, and each refuses the
//! other's:
//!
//! | policy | wants the name to be | refuses with |
//! |---|---|---|
//! | `NewField` | vacant | [`EditError::FieldNameTaken`](crate::edit::EditError::FieldNameTaken) |
//! | `AdditionalWidget` | an existing terminal of a matching type | [`EditError::FieldNotFound`](crate::edit::EditError::FieldNotFound) |
//!
//! **Neither ever falls back to the other**, and that is a requirement rather
//! than a nicety: the operator pressed a *different key on purpose*. A
//! `Ctrl+Shift+V` that silently became a `Ctrl+V` because the field happened
//! not to exist in this document would produce an independent field where the
//! operator asked for a linked one — a difference nothing on screen shows
//! until somebody types in one and the other does not follow.
//!
//! # Which of the two is the high-fidelity path (it is not the obvious one)
//!
//! `AdditionalWidget` **does not touch the field object at all**. It appends
//! a widget through
//! [`merge_widget_into_field`](crate::edit::EditSession) — the §12.7.3.2
//! mechanism that makes two widgets sharing a fully-qualified name two views
//! of one field — so `/DA`, `/Q`, `/V`, `/DV`, `/Ff` and `/AA` are inherited
//! *exactly*, because they are not copied at all. It is the same field.
//!
//! `NewField` is the lossy one, and everything in this module exists to make
//! it lossless anyway.
//!
//! # Multi-widget fields carry as ONE unit
//!
//! A radio group is one field with N widgets, each carrying its own export
//! value as the on-state key in its `/AP /N` subdictionary (§12.7.4.2.3).
//! Copying "a radio button" and getting one widget back would produce, on
//! paste, either a group of one or N independent fields — neither of which is
//! what was on screen.
//!
//! So [`FieldClip`] carries **every** widget, in `/Kids` order, with each
//! one's rectangle. On paste:
//!
//! - **one widget** → the caller's `rect` is used verbatim. It is the
//!   rectangle the operator drew.
//! - **more than one** → the group is **translated** so that widget 0's
//!   lower-left corner lands on `rect`'s lower-left, and every widget keeps
//!   its own size and its offset from widget 0. The caller's rect *size* is
//!   ignored, and that is disclosed.
//!
//! Translation rather than scale-to-fit, because a radio group's geometry is
//! part of its meaning: which button is above which, and how far apart. A
//! best-fit rescale of a two-column group into a rectangle the operator drew
//! by eye is a guess, and it is a guess that looks deliberate.
//!
//! # Spec obligations this module discharges on paste
//!
//! - **§12.7.2 Table 218 `/CO`** — *"Required if any field has an `AA` dict
//!   with a `C` entry"*. A pasted field carrying a calculate action is
//!   appended to `/AcroForm /CO`, or the destination becomes non-conformant
//!   the moment the clip lands. pdfcer still never *executes* it (NF4).
//! - **§12.7.3.3 `/DA`** — the appearance string names a font that must
//!   resolve in `/AcroForm /DR /Font`. The clip carries that font entry and
//!   the paste installs it, renaming and rewriting the `/DA` when the
//!   destination already uses the name for a *different* font.
//! - **§12.7.2 Table 219 `/SigFlags`** — bit 1 (`SignaturesExist`) is set
//!   when a `/Sig` field is planted.
//! - **§12.7.4.5** — a signature's `/V` covers a byte range *of the document
//!   it was made in*. It is dropped unconditionally on paste; carrying it
//!   would put a signature dictionary in a file it does not describe.
//! - **R101** — a widget kid carries no `/T`, `/FT` or `/Kids`. Enforced by
//!   `merge_widget_into_field`, and by this module's own key split.
//! - **R104** — a pasted widget is appended to the page's `/Annots`, which
//!   puts it last in an explicit tab order. Disclosed, never "fixed" by
//!   re-sorting an array pdfcer did not logically change.
//! - **R105** — a pasted field carries an accessibility name or an explicit
//!   declination. [`PasteTooltip`] makes that a decision the caller states,
//!   including the decision to reuse the source's.
//!
//! # Serialisation
//!
//! [`FieldClip::to_bytes`] mirrors
//! [`ObjectClip::to_bytes`](crate::vector::ObjectClip::to_bytes) exactly:
//! magic, version, then length-prefixed values whose *object* payloads are
//! written as PDF syntax by the crate's own
//! [`write_object`](crate::writer::serialize::write_object) and read back by
//! its own [`Parser`](crate::parser::Parser). The COS grammar has one
//! implementation on each side; this module does not add a second.
//!
//! Unlike `ObjectClip`, **everything a `FieldClip` holds survives the round
//! trip** — there is no annotations-shaped hole, because a field clip is
//! dictionaries and streams all the way down rather than rich Rust enums.
//!
//! [`FieldClip::from_bytes`] parses untrusted bytes and is therefore fuzzed
//! (`fuzz/fuzz_targets/clip_from_bytes.rs`), depth-guarded by the parser's own
//! `MAX_NESTING_DEPTH`, and count-guarded by [`MAX_CLIP_OBJECTS`] and
//! [`MAX_CLIP_WIDGETS`] so a hostile length prefix cannot make it allocate.

use crate::forms::{self, ButtonKind, FieldType};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object, Stream};
use crate::page_tree::Rect;
use crate::span::ByteSpan;
use crate::vector::clip::{ClipError, ClipObject};
use crate::view::DocumentView;
use std::collections::BTreeMap;

/// The field-clipboard format version [`FieldClip::to_bytes`] writes.
///
/// Deliberately **separate** from
/// [`CLIP_VERSION`](crate::vector::clip::CLIP_VERSION): the two payloads have
/// different magic, different shapes and different reasons to change, so one
/// shared number would force a version bump on both whenever either moved.
pub const FIELD_CLIP_VERSION: u32 = 1;

/// The twelve-byte signature every field-clip payload starts with.
///
/// Twelve bytes, matching `CLIP_MAGIC`'s width, so a shell that sniffs the
/// first sixteen bytes of an unknown clipboard payload can tell the two pdfcer
/// formats apart with one comparison each.
pub const FIELD_CLIP_MAGIC: &[u8; 12] = b"PDFCEFLD\x00\x00\x00\x01";

/// How many objects a clip's owned closure may hold.
///
/// A form field's closure is small — an appearance stream or two, an `/AA`
/// action dictionary, a font. Four thousand is far above anything legitimate
/// and far below anything that costs memory, which is the shape a ceiling
/// should have: it never fires for real work and it caps a hostile file.
pub const MAX_CLIP_OBJECTS: usize = 4096;

/// How many widgets one field may carry onto the clipboard.
///
/// A radio group with more than a few dozen buttons is already unusual; five
/// hundred is a guard against a `/Kids` array built to make a reader allocate,
/// not a limit an operator will meet.
pub const MAX_CLIP_WIDGETS: usize = 512;

/// How deep the closure walker follows a value tree before giving up.
///
/// Matches the spirit of `pageops`' `MAX_COPY_DEPTH`: a hostile 200-deep array
/// costs the operator one degraded value, not the copy.
const MAX_CLIP_DEPTH: usize = 32;

/// Field-half keys that are the field's **identity or structure**, and are
/// therefore never carried.
///
/// `/T` is the name the policy supplies. `/Parent` and `/Kids` describe where
/// the field sat in the source's field tree, which the paste rebuilds.
const FIELD_IDENTITY_KEYS: &[&[u8]] = &[b"T", b"Parent", b"Kids", b"Type"];

/// Widget-half keys dropped at copy time because they name something that
/// exists only in the source document.
///
/// See the module docs for the per-key reasoning — each of these is a
/// deliberate drop with a stated cause, not a shortcut.
const WIDGET_SOURCE_BOUND_KEYS: &[&[u8]] = &[
    b"P",
    b"Parent",
    b"StructParent",
    b"NM",
    b"M",
    b"Type",
    b"Subtype",
];

/// The field-half keys a clip carries.
///
/// Derived from [`crate::forms_author::FIELD_ONLY_KEYS`] plus the three
/// variable-text/permission keys that live on a field but are in neither of
/// that module's two lists (`/DA` because a *merged* field carries it as both
/// halves, `/TU` and `/Lock`/`/SV` because they were never part of the
/// promotion question those lists answer).
const FIELD_CARRIED_KEYS: &[&[u8]] = &[
    b"FT", b"Ff", b"V", b"DV", b"AA", b"Opt", b"MaxLen", b"Q", b"TI", b"I", b"RV", b"DS", b"TM",
    b"TU", b"DA", b"Lock", b"SV",
];

/// One widget on the clipboard: its dictionary and where it sat.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct FieldClipWidget {
    /// The widget dictionary, with clip-local references and every
    /// source-bound key already removed.
    pub(crate) dict: Dict,
    /// The widget's rectangle in the **source** page's user space.
    ///
    /// Carried rather than recomputed because it is what makes a multi-widget
    /// paste preserve the group's shape: the offsets between these rectangles
    /// are the group's geometry.
    pub(crate) rect: Rect,
}

impl FieldClipWidget {
    /// The rectangle this widget occupied in its source page.
    #[must_use]
    pub const fn rect(&self) -> Rect {
        self.rect
    }

    /// Whether this widget carries a baked `/AP` appearance.
    #[must_use]
    pub fn has_appearance(&self) -> bool {
        self.dict.contains_key(b"AP")
    }
}

/// A copied form field — everything it IS, minus its identity (`Pass 167.0`).
///
/// Opaque by intent, exactly as the consuming shell asked for: a shell moves
/// one of these around, hands it back to
/// [`paste_field`](crate::edit::EditSession::paste_field), and asks it only
/// the questions a paste UI needs — what type is it, does it carry a script,
/// what was it called. Its internals are `pub(crate)` for this crate's own
/// planner, not as a poke-at-it surface.
///
/// # Examples
///
/// ```no_run
/// use pdfcer_core::document::Document;
/// use pdfcer_core::edit::EditSession;
/// use std::path::Path;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let session = EditSession::new(Document::load(Path::new("form.pdf"))?);
/// let clip = session.copy_field("TitleBlock.Revision")?;
/// // Carry it to another document, another process, another day.
/// std::fs::write("revision.fieldclip", clip.to_bytes())?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct FieldClip {
    /// The format version this clip was written at.
    pub(crate) version: u32,
    /// The field half — never `/T`, `/Parent` or `/Kids`.
    pub(crate) field: Dict,
    /// The widget halves, in the source field's `/Kids` order (or the single
    /// merged widget, for a Shape A field).
    pub(crate) widgets: Vec<FieldClipWidget>,
    /// The owned object closure, keyed by clip-local object number.
    pub(crate) objects: BTreeMap<u32, ClipObject>,
    /// The `/AcroForm /DR /Font` entry the field's `/DA` names: the resource
    /// name as written in the `/DA`, and the clip-local object holding the
    /// font dictionary.
    pub(crate) da_font: Option<(Vec<u8>, u32)>,
    /// The source field's fully-qualified name — for seeding a rename box.
    pub(crate) source_name: String,
    /// The source field's resolved `/FT`.
    pub(crate) field_type: Option<FieldType>,
    /// For a `/Btn`, which kind of button. Decisive for paste: a check box
    /// and a radio group are both `/FT /Btn` and are not interchangeable.
    pub(crate) button_kind: Option<ButtonKind>,
}

impl FieldClip {
    /// The source field's fully-qualified name.
    ///
    /// Not the name the paste will use — that is the operator's decision.
    /// This is what a rename control should be *seeded* with.
    #[must_use]
    pub fn source_name(&self) -> &str {
        &self.source_name
    }

    /// The clipped field's type, or `None` for a malformed source field with
    /// no resolvable `/FT`.
    ///
    /// A `None` here is a refusal on paste, not a guess: §12.7.3.1 Table 220
    /// makes `/FT` required for a terminal field, and pdfcer does not invent
    /// one.
    #[must_use]
    pub const fn field_type(&self) -> Option<FieldType> {
        self.field_type
    }

    /// For a button field, which kind of button.
    #[must_use]
    pub const fn button_kind(&self) -> Option<ButtonKind> {
        self.button_kind
    }

    /// How many widgets travel with this clip.
    ///
    /// More than one means the paste places a **group** and ignores the
    /// caller's rectangle size — see the module docs.
    #[must_use]
    pub fn widget_count(&self) -> usize {
        self.widgets.len()
    }

    /// The widgets, in the source field's `/Kids` order.
    #[must_use]
    pub fn widgets(&self) -> &[FieldClipWidget] {
        &self.widgets
    }

    /// Whether the clip carries an `/AA` additional-actions dictionary — a
    /// format, calculate, validate or keystroke script.
    ///
    /// **Ask this BEFORE the press, not after.** A calculation that arrives
    /// with a pasted field is invisible on the page, and a calculation that
    /// was silently dropped is equally invisible: rule 4 (*fuzzy, never
    /// sneaky*) is satisfied by disclosing which happened, and this is the
    /// accessor that lets a shell disclose it while the operator can still
    /// choose.
    ///
    /// pdfcer never executes the script either way (decision 008 §5.1, NF4).
    #[must_use]
    pub fn carries_actions(&self) -> bool {
        self.field.contains_key(b"AA")
            || self.widgets.iter().any(|w| w.dict.contains_key(b"AA"))
            || self.widgets.iter().any(|w| w.dict.contains_key(b"A"))
    }

    /// Whether the clip's `/AA` includes a **calculate** (`/C`) action.
    ///
    /// Separated from [`Self::carries_actions`] because it is the one that
    /// obliges the destination to grow an `/AcroForm /CO` entry (§12.7.2
    /// Table 218), and because "this field is part of a calculation chain" is
    /// a materially different warning from "this field reformats what you
    /// type".
    #[must_use]
    pub fn carries_calculation(&self) -> bool {
        self.field
            .get(b"AA")
            .and_then(Object::as_dict)
            .is_some_and(|aa| aa.contains_key(b"C"))
    }

    /// Whether the clip carries a field **value** (`/V`).
    #[must_use]
    pub fn carries_value(&self) -> bool {
        self.field.contains_key(b"V")
    }

    /// Whether the clip carries an accessibility name (`/TU`), and what it is.
    ///
    /// [`PasteTooltip::Carry`] is only a meaningful answer when this is
    /// `Some`; when it is `None`, `Carry` degrades to a declination and says
    /// so.
    #[must_use]
    pub fn tooltip(&self) -> Option<&[u8]> {
        match self.field.get(b"TU") {
            Some(Object::String(bytes)) => Some(bytes),
            _ => None,
        }
    }

    /// The `/AcroForm /DR /Font` resource name the field's `/DA` names, when
    /// the clip carries that font.
    ///
    /// A shell can show *"this field's font travels with it"* rather than
    /// leaving the operator to discover on paste that a 14 pt face came back
    /// as the destination's default.
    #[must_use]
    pub fn carried_font(&self) -> Option<&[u8]> {
        self.da_font.as_ref().map(|(name, _)| name.as_slice())
    }

    /// How many objects the clip's owned closure holds.
    #[must_use]
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    /// The union of the source widgets' rectangles.
    ///
    /// `None` for a value-only field with no on-page presence — legal under
    /// Table 220 and not pasteable as a widget, which the paste refuses by
    /// name rather than planting an invisible box.
    #[must_use]
    pub fn bbox(&self) -> Option<Rect> {
        let mut it = self.widgets.iter();
        let first = it.next()?.rect;
        Some(it.fold(first, |acc, w| Rect {
            llx: acc.llx.min(w.rect.llx),
            lly: acc.lly.min(w.rect.lly),
            urx: acc.urx.max(w.rect.urx),
            ury: acc.ury.max(w.rect.ury),
        }))
    }

    /// Serialise the clip so it survives leaving this process (`Pass 167.0`).
    ///
    /// [`FIELD_CLIP_MAGIC`], a version, then length-prefixed values. Object
    /// values go through the crate's own
    /// [`write_object`](crate::writer::serialize::write_object) and come back
    /// through its own [`Parser`](crate::parser::Parser), so the COS grammar
    /// has exactly one implementation on each side.
    ///
    /// **Everything survives.** Unlike `ObjectClip`, which drops its
    /// annotations because they are modelled as rich Rust enums, a field clip
    /// is dictionaries and streams — the same things a PDF file is made of.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(FIELD_CLIP_MAGIC);
        put_u32(&mut out, self.version);
        put_bytes(&mut out, self.source_name.as_bytes());
        out.push(field_type_tag(self.field_type));
        out.push(button_kind_tag(self.button_kind));

        put_object(&mut out, &Object::Dict(self.field.clone()));

        put_u32(
            &mut out,
            u32::try_from(self.widgets.len()).unwrap_or(u32::MAX),
        );
        for widget in &self.widgets {
            put_object(&mut out, &Object::Dict(widget.dict.clone()));
            put_f64(&mut out, widget.rect.llx);
            put_f64(&mut out, widget.rect.lly);
            put_f64(&mut out, widget.rect.urx);
            put_f64(&mut out, widget.rect.ury);
        }

        match &self.da_font {
            Some((name, object)) => {
                out.push(1);
                put_bytes(&mut out, name);
                put_u32(&mut out, *object);
            }
            None => out.push(0),
        }

        put_u32(
            &mut out,
            u32::try_from(self.objects.len()).unwrap_or(u32::MAX),
        );
        for (&id, object) in &self.objects {
            put_u32(&mut out, id);
            // A stream's DICTIONARY is what is written; its payload travels
            // beside it, because `Object::Stream` carries a span into a buffer
            // that does not exist here.
            let (value, payload): (Object, Option<&Vec<u8>>) = match &object.value {
                Object::Stream(stream) => {
                    (Object::Dict(stream.dict.clone()), object.payload.as_ref())
                }
                other => (other.clone(), None),
            };
            put_object(&mut out, &value);
            match payload {
                Some(bytes) => {
                    out.push(1);
                    put_bytes(&mut out, bytes);
                }
                None => out.push(0),
            }
        }
        out
    }

    /// Parse a payload written by [`Self::to_bytes`].
    ///
    /// # Errors
    ///
    /// [`ClipError::NotAClip`] when the magic does not match — checked first,
    /// so an unrelated payload (an `ObjectClip`, a JPEG, a truncated download)
    /// is refused with a sentence rather than with whatever a length prefix
    /// read out of the wrong bytes. [`ClipError::NewerFormat`] for a payload
    /// from a newer build, refused rather than half-understood.
    /// [`ClipError::Truncated`] for a short read, [`ClipError::Content`] for a
    /// value that is not COS syntax, and [`ClipError::ClipTooLarge`] when a
    /// length prefix claims more widgets or objects than
    /// [`MAX_CLIP_WIDGETS`]/[`MAX_CLIP_OBJECTS`] permit.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ClipError> {
        let mut r = Reader::new(bytes);
        if r.take(FIELD_CLIP_MAGIC.len())? != FIELD_CLIP_MAGIC.as_slice() {
            return Err(ClipError::NotAClip);
        }
        let version = r.u32()?;
        if version > FIELD_CLIP_VERSION {
            return Err(ClipError::NewerFormat {
                found: version,
                supported: FIELD_CLIP_VERSION,
            });
        }
        let source_name = String::from_utf8_lossy(&r.bytes()?).into_owned();
        let field_type = field_type_of_tag(r.byte()?);
        let button_kind = button_kind_of_tag(r.byte()?);

        let field =
            r.object()?.as_dict().cloned().ok_or_else(|| {
                ClipError::Content("the field half is not a dictionary".to_owned())
            })?;

        let widget_count = r.u32()? as usize;
        // Checked BEFORE `with_capacity`: a hostile prefix must not be able to
        // make this allocate before it is refused.
        if widget_count > MAX_CLIP_WIDGETS {
            return Err(ClipError::ClipTooLarge {
                found: widget_count,
                limit: MAX_CLIP_WIDGETS,
            });
        }
        let mut widgets = Vec::with_capacity(widget_count);
        for _ in 0..widget_count {
            let dict = r.object()?.as_dict().cloned().ok_or_else(|| {
                ClipError::Content("a widget half is not a dictionary".to_owned())
            })?;
            let rect = Rect {
                llx: r.f64()?,
                lly: r.f64()?,
                urx: r.f64()?,
                ury: r.f64()?,
            };
            widgets.push(FieldClipWidget { dict, rect });
        }

        let da_font = if r.byte()? == 1 {
            let name = r.bytes()?;
            let object = r.u32()?;
            Some((name, object))
        } else {
            None
        };

        let object_count = r.u32()? as usize;
        if object_count > MAX_CLIP_OBJECTS {
            return Err(ClipError::ClipTooLarge {
                found: object_count,
                limit: MAX_CLIP_OBJECTS,
            });
        }
        let mut objects = BTreeMap::new();
        for _ in 0..object_count {
            let id = r.u32()?;
            let value = r.object()?;
            let payload = if r.byte()? == 1 {
                Some(r.bytes()?)
            } else {
                None
            };
            // A stream is reconstructed from its dictionary plus its payload;
            // the span is meaningless by construction, exactly as it was on
            // the way out.
            let value = match (&value, &payload) {
                (Object::Dict(dict), Some(bytes)) => Object::Stream(Stream {
                    dict: dict.clone(),
                    data_span: ByteSpan::new(0, bytes.len()),
                }),
                _ => value,
            };
            objects.insert(id, ClipObject { value, payload });
        }

        Ok(Self {
            version,
            field,
            widgets,
            objects,
            da_font,
            source_name,
            field_type,
            button_kind,
        })
    }
}

/// What the accessibility name (`/TU`) of a pasted field should be (R105).
///
/// # Why a fourth answer exists here and not on [`TooltipChoice`]
///
/// [`TooltipChoice`](crate::edit::TooltipChoice) has three answers — supply
/// one, decline one, or have not decided (which is refused). That is the right
/// set for *creating* a field from nothing.
///
/// A paste has a fourth possibility that creation does not: the source field
/// already has an accessibility name, and reusing it may well be right,
/// because the operator is copying **their own field**. That is a legitimate
/// explicit answer, so R105 is satisfied — the rule requires a decision, not a
/// freshly typed string.
///
/// It is a separate enum rather than a fourth `TooltipChoice` variant because
/// adding a variant to a public exhaustively-matched enum breaks every
/// downstream `match`, and "carry the source's" is meaningless to the four
/// creation verbs that would then have to handle it.
///
/// **Two of the same name is still an accessibility defect**, so
/// [`Self::Carry`] discloses that it fired — two fields announcing themselves
/// identically to a screen reader is exactly the kind of thing a sighted
/// operator cannot see.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum PasteTooltip {
    /// Nobody has decided. **Refused** — R105 is not satisfied by a default.
    #[default]
    Undecided,
    /// Reuse the clip's own `/TU`. Degrades to a declination, disclosed, when
    /// the clip carries none.
    Carry,
    /// A freshly chosen accessibility name.
    Text(String),
    /// Explicitly declined. No `/TU` is written, and the declination is
    /// disclosed.
    Declined,
}

/// Which of the operator's two paste chords is being performed.
///
/// See the module docs for why these are two policies on one verb, and why
/// neither ever falls back to the other.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FieldPastePolicy {
    /// **Ctrl+V** — a new, independent field.
    ///
    /// Carries `/FT`, `/Ff`, `/DA`, `/Q`, `/DV`, `/MaxLen`, `/Opt`, `/MK`,
    /// `/BS` and the widget appearance; carries `/V` and `/AA` only when
    /// asked. Refused when `name` is already taken
    /// ([`EditError::FieldNameTaken`](crate::edit::EditError::FieldNameTaken))
    /// — never auto-suffixed, because an engine-invented name is a name
    /// nobody chose, and the operator can see the candidate before pressing.
    NewField {
        /// The new field's fully-qualified name. A period separates levels
        /// (§12.7.3.2) and any missing grouping ancestors are created.
        name: String,
        /// The accessibility-name decision (R105).
        tooltip: PasteTooltip,
        /// Carry the source field's `/V`.
        ///
        /// Explicit rather than inferred: a value is *content*. A copied
        /// "Revision" field arriving pre-filled with the source drawing's
        /// revision is a wrong answer that looks like a right one.
        copy_value: bool,
        /// Carry the source field's `/AA` (and the widget's `/A`).
        ///
        /// Explicit rather than inferred: an action is *behaviour*. A
        /// calculation that references fields absent from the destination is
        /// inert, and its inertness is invisible.
        copy_actions: bool,
    },
    /// **Ctrl+Shift+V** — another widget of a field that already exists here.
    ///
    /// The field object is not touched, so `/DA`, `/Q`, `/V`, `/DV`, `/Ff`
    /// and `/AA` are shared exactly (§12.7.3.2 — it *is* the same field).
    /// Refused when `existing` names nothing in this document
    /// ([`EditError::FieldNotFound`](crate::edit::EditError::FieldNotFound)),
    /// and refused on a type mismatch, which is Acrobat's own behaviour at
    /// this junction.
    AdditionalWidget {
        /// The fully-qualified name of the field to attach to.
        existing: String,
    },
}

impl FieldPastePolicy {
    /// The field name this policy targets, whichever branch it is.
    #[must_use]
    pub fn target_name(&self) -> &str {
        match self {
            Self::NewField { name, .. } => name,
            Self::AdditionalWidget { existing } => existing,
        }
    }
}

// ---------------------------------------------------------------------------
// Copy side — building the clip
// ---------------------------------------------------------------------------

/// Build a clip from a parsed field and the document it lives in.
///
/// `field` must be a terminal field from `form`; `view` must be the same
/// document's view (a session's [`view`](crate::edit::EditSession::view), so
/// authored appearance spans resolve into the staging buffer rather than off
/// the end of the base file).
///
/// # Errors
///
/// [`ClipError::ClipTooLarge`] when the field's closure exceeds
/// [`MAX_CLIP_OBJECTS`], or when it carries more than [`MAX_CLIP_WIDGETS`]
/// widgets.
pub(crate) fn build_field_clip(
    view: &DocumentView<'_>,
    form: &forms::AcroForm,
    field: &forms::Field,
) -> Result<FieldClip, ClipError> {
    let graph = view.graph();
    let mut closure = Closure::new(view);

    // ---- the field half -------------------------------------------------
    //
    // Read from the RAW dictionary, not from `forms::Field`. The read
    // projection resolves inherited attributes, so `field.flags` may hold a
    // value the field never carried itself — planting that as an own `/Ff`
    // would materialise an inheritance the operator never chose. And it does
    // not model `/AA`'s contents, `/Lock`, `/SV` or `/MK` at all.
    let source_dict = graph
        .resolved(field.id)
        .as_dict()
        .cloned()
        .unwrap_or_default();
    let mut carried = Dict::new();
    for key in FIELD_CARRIED_KEYS {
        if let Some(value) = source_dict.get(key) {
            carried.insert(Name::from(*key), closure.take(value, 0)?);
        }
    }
    // `/FT` is the one key a merged Shape A field may have inherited AND that
    // the paste cannot proceed without, so it is materialised from the read
    // projection when the raw dictionary is silent. That is a deliberate
    // exception to the paragraph above: an absent `/FT` is not an inheritance
    // the operator chose, it is a field that has no type of its own and whose
    // type came from a parent that is not travelling.
    if !carried.contains_key(b"FT")
        && let Some(ft) = field.field_type
    {
        carried.insert(Name::from(b"FT"), Object::Name(Name::from(ft.as_ft_name())));
    }
    for key in FIELD_IDENTITY_KEYS {
        carried.remove(key);
    }

    // ---- the widget halves ----------------------------------------------
    if field.widgets.len() > MAX_CLIP_WIDGETS {
        return Err(ClipError::ClipTooLarge {
            found: field.widgets.len(),
            limit: MAX_CLIP_WIDGETS,
        });
    }
    let mut widgets = Vec::with_capacity(field.widgets.len());
    for widget in &field.widgets {
        let raw = graph
            .resolved(widget.id)
            .as_dict()
            .cloned()
            .unwrap_or_default();
        let mut dict = Dict::new();
        for (key, value) in raw.iter() {
            let k = key.as_bytes();
            if WIDGET_SOURCE_BOUND_KEYS.contains(&k) {
                continue;
            }
            // A MERGED (Shape A) field is one dictionary wearing both hats,
            // and this half is about to be only the widget. Its field keys
            // are carried above; its identity keys are the paste's to supply.
            //
            // ★ `/DA` and `/T` are the two that BIT, and both bit the same
            // way: the paste writes the field half first and then folds the
            // widget half over it, so anything left here that also belongs on
            // the field SILENTLY WINS. A leftover `/T` made every paste come
            // back under the source's name; a leftover `/DA` undid the
            // font-resource rename the paste had just performed and disclosed.
            // Neither showed up as an error -- the document was well formed
            // and wrong.
            if FIELD_CARRIED_KEYS.contains(&k) || FIELD_IDENTITY_KEYS.contains(&k) {
                continue;
            }
            dict.insert(key.clone(), closure.take(value, 0)?);
        }
        widgets.push(FieldClipWidget {
            dict,
            rect: widget.rect.unwrap_or(Rect {
                llx: 0.0,
                lly: 0.0,
                urx: 0.0,
                ury: 0.0,
            }),
        });
    }

    // ---- the /DA font ----------------------------------------------------
    //
    // §12.7.3.3: the `/DA` names a font that must resolve in the AcroForm's
    // `/DR /Font`. Carrying the `/DA` without the font it names produces a
    // field whose appearance the DESTINATION cannot regenerate — which is
    // precisely the "it looks wrong after paste" the consuming shell reported.
    let da_font = carried
        .get(b"DA")
        .and_then(|da| match graph.resolve(da) {
            Object::String(bytes) => Some(bytes.clone()),
            _ => None,
        })
        .and_then(|da| crate::vartext::parse_default_appearance(&da).ok())
        .map(|parsed| parsed.font_name)
        .and_then(|name| {
            let entry = dr_font_entry(graph, form, &name)?;
            Some((name, entry))
        });
    let da_font = match da_font {
        Some((name, value)) => {
            let interned = closure.take(&value, 0)?;
            // The font must be an INDIRECT object in the clip so the paste can
            // install it into `/DR /Font` by reference rather than inlining a
            // whole (possibly embedded) font into a dictionary.
            let number = match interned {
                Object::Reference(id) => id.num,
                direct => closure.adopt(direct)?,
            };
            Some((name, number))
        }
        None => None,
    };

    Ok(FieldClip {
        version: FIELD_CLIP_VERSION,
        field: carried,
        widgets,
        objects: closure.objects,
        da_font,
        source_name: field.fully_qualified_name.clone(),
        field_type: field.field_type,
        button_kind: field.button_kind,
    })
}

/// The `/AcroForm /DR /Font /<name>` value, unresolved, so an indirect entry
/// stays indirect and the closure walker sees a reference to intern.
fn dr_font_entry<G: ObjectGraph + ?Sized>(
    graph: &G,
    form: &forms::AcroForm,
    name: &[u8],
) -> Option<Object> {
    if !form.has_default_resources {
        return None;
    }
    let catalog = graph.catalog_dict()?;
    let acroform = graph.resolve(catalog.get(b"AcroForm")?).as_dict()?;
    let dr = graph.resolve(acroform.get(b"DR")?).as_dict()?;
    let fonts = graph.resolve(dr.get(b"Font")?).as_dict()?;
    fonts.get(name).cloned()
}

/// The owned-closure builder: interns every object a carried value reaches,
/// rewriting references into clip-local numbering.
///
/// A worklist would be over-engineering here — a field's closure is shallow
/// and small — but the depth guard is not optional: `/AA` and `/AP` are
/// operator-supplied structures from an untrusted file, and §7.3.7 permits
/// arbitrary nesting.
pub(crate) struct Closure<'a> {
    view: &'a DocumentView<'a>,
    pub(crate) objects: BTreeMap<u32, ClipObject>,
    mapping: BTreeMap<ObjId, u32>,
    next: u32,
}

impl<'a> Closure<'a> {
    pub(crate) fn new(view: &'a DocumentView<'a>) -> Self {
        Self {
            view,
            objects: BTreeMap::new(),
            mapping: BTreeMap::new(),
            next: 1,
        }
    }

    /// Store a value that has no source object behind it, returning its
    /// clip-local number.
    pub(crate) fn adopt(&mut self, value: Object) -> Result<u32, ClipError> {
        let number = self.reserve()?;
        self.objects.insert(
            number,
            ClipObject {
                value,
                payload: None,
            },
        );
        Ok(number)
    }

    fn reserve(&mut self) -> Result<u32, ClipError> {
        if self.objects.len() >= MAX_CLIP_OBJECTS {
            return Err(ClipError::ClipTooLarge {
                found: self.objects.len() + 1,
                limit: MAX_CLIP_OBJECTS,
            });
        }
        let number = self.next;
        self.next = self.next.saturating_add(1);
        Ok(number)
    }

    /// Copy one value tree into the clip, rewriting references.
    ///
    /// Exceeding [`MAX_CLIP_DEPTH`] degrades that sub-tree to `null` rather
    /// than failing the copy, matching `pageops`' posture: a hostile nesting
    /// costs the operator one broken value, not the operation.
    pub(crate) fn take(&mut self, value: &Object, depth: usize) -> Result<Object, ClipError> {
        if depth > MAX_CLIP_DEPTH {
            return Ok(Object::Null);
        }
        Ok(match value {
            Object::Reference(id) => Object::Reference(ObjId::new(self.intern(*id)?, 0)),
            Object::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(self.take(item, depth + 1)?);
                }
                Object::Array(out)
            }
            Object::Dict(dict) => Object::Dict(self.take_dict(dict, depth)?),
            // A stream reached as a DIRECT value cannot occur in a conforming
            // file (§7.3.8.1 — all streams are indirect objects), but a
            // recovered one can, so it is carried by its dictionary rather
            // than dropped.
            Object::Stream(stream) => Object::Dict(self.take_dict(&stream.dict, depth)?),
            other => other.clone(),
        })
    }

    fn take_dict(&mut self, dict: &Dict, depth: usize) -> Result<Dict, ClipError> {
        let mut out = Dict::new();
        for (key, value) in dict.iter() {
            out.insert(key.clone(), self.take(value, depth + 1)?);
        }
        Ok(out)
    }

    /// Intern a source object, copying it (and everything it reaches) on
    /// first sighting.
    ///
    /// The mapping entry is written **before** the value is walked, so a
    /// reference cycle — `/Parent` chains and `/AA` action chains both make
    /// them — terminates instead of recursing forever.
    fn intern(&mut self, id: ObjId) -> Result<u32, ClipError> {
        if let Some(existing) = self.mapping.get(&id) {
            return Ok(*existing);
        }
        let number = self.reserve()?;
        self.mapping.insert(id, number);
        // A placeholder so the ceiling counts this object while it is being
        // built, and so a cycle back to it resolves to something.
        self.objects.insert(
            number,
            ClipObject {
                value: Object::Null,
                payload: None,
            },
        );
        let source = self.view.graph().value(id).cloned();
        let entry = match source {
            Some(Object::Stream(stream)) => {
                // RAW bytes, and the dictionary unchanged: `/Filter` and
                // `/Length` still describe them, so the pair stays valid
                // without this module knowing a single codec.
                let payload = self
                    .view
                    .slice(stream.data_span)
                    .map(<[u8]>::to_vec)
                    .unwrap_or_default();
                let dict = self.take_dict(&stream.dict, 0)?;
                ClipObject {
                    value: Object::Stream(Stream {
                        dict,
                        data_span: ByteSpan::new(0, payload.len()),
                    }),
                    payload: Some(payload),
                }
            }
            Some(other) => ClipObject {
                value: self.take(&other, 0)?,
                payload: None,
            },
            // A dangling reference resolves to null (§7.3.10) rather than
            // failing the copy — the source document already renders that way.
            None => ClipObject {
                value: Object::Null,
                payload: None,
            },
        };
        self.objects.insert(number, entry);
        Ok(number)
    }
}

// ---------------------------------------------------------------------------
// Byte-level primitives — deliberately identical in shape to `vector::clip`'s
// ---------------------------------------------------------------------------

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// Bit-exact, not decimal. A rectangle that changed in the last place on every
/// copy/paste cycle would drift a widget visibly after enough of them.
fn put_f64(out: &mut Vec<u8>, v: f64) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    put_u32(out, u32::try_from(bytes.len()).unwrap_or(u32::MAX));
    out.extend_from_slice(bytes);
}

/// Write a COS value as PDF syntax, through the crate's own serialiser.
fn put_object(out: &mut Vec<u8>, value: &Object) {
    use crate::writer::encoder::IdentityEncoder;
    use crate::writer::serialize::write_object;
    let mut encoded = Vec::new();
    write_object(&mut encoded, value, ObjId::new(0, 0), &[], &IdentityEncoder);
    put_bytes(out, &encoded);
}

const fn field_type_tag(ft: Option<FieldType>) -> u8 {
    match ft {
        None => 0,
        Some(FieldType::Button) => 1,
        Some(FieldType::Text) => 2,
        Some(FieldType::Choice) => 3,
        Some(FieldType::Signature) => 4,
    }
}

const fn field_type_of_tag(tag: u8) -> Option<FieldType> {
    match tag {
        1 => Some(FieldType::Button),
        2 => Some(FieldType::Text),
        3 => Some(FieldType::Choice),
        4 => Some(FieldType::Signature),
        _ => None,
    }
}

const fn button_kind_tag(kind: Option<ButtonKind>) -> u8 {
    match kind {
        None => 0,
        Some(ButtonKind::Push) => 1,
        Some(ButtonKind::Check) => 2,
        Some(ButtonKind::Radio) => 3,
    }
}

const fn button_kind_of_tag(tag: u8) -> Option<ButtonKind> {
    match tag {
        1 => Some(ButtonKind::Push),
        2 => Some(ButtonKind::Check),
        3 => Some(ButtonKind::Radio),
        _ => None,
    }
}

/// A bounds-checked cursor over a clip payload.
///
/// Every read is `checked_add` + `get`, so no arithmetic on a hostile length
/// prefix can wrap into an in-bounds slice.
struct Reader<'a> {
    buf: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    const fn new(buf: &'a [u8]) -> Self {
        Self { buf, at: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], ClipError> {
        let end = self.at.checked_add(n).ok_or(ClipError::Truncated)?;
        let slice = self.buf.get(self.at..end).ok_or(ClipError::Truncated)?;
        self.at = end;
        Ok(slice)
    }

    fn byte(&mut self) -> Result<u8, ClipError> {
        Ok(self.take(1)?.first().copied().unwrap_or(0))
    }

    fn u32(&mut self) -> Result<u32, ClipError> {
        // `try_into` rather than four indexes: `take` already guaranteed the
        // length, and the conversion says so to the compiler instead of to a
        // reader. There is no path here that panics.
        let b: [u8; 4] = self.take(4)?.try_into().map_err(|_| ClipError::Truncated)?;
        Ok(u32::from_le_bytes(b))
    }

    fn f64(&mut self) -> Result<f64, ClipError> {
        let b: [u8; 8] = self.take(8)?.try_into().map_err(|_| ClipError::Truncated)?;
        Ok(f64::from_le_bytes(b))
    }

    fn bytes(&mut self) -> Result<Vec<u8>, ClipError> {
        let len = self.u32()? as usize;
        Ok(self.take(len)?.to_vec())
    }

    fn object(&mut self) -> Result<Object, ClipError> {
        let encoded = self.bytes()?;
        crate::parser::Parser::at(&encoded, 0)
            .parse_object()
            .map_err(|e| ClipError::Content(e.to_string()))
    }
}

impl FieldClipWidget {
    /// This widget's dictionary, with clip-local references.
    pub(crate) const fn dict(&self) -> &Dict {
        &self.dict
    }
}

impl FieldClip {
    /// The field half, with clip-local references.
    pub(crate) const fn field_dict(&self) -> &Dict {
        &self.field
    }

    /// The owned object closure, for the session's materialiser.
    pub(crate) const fn objects(&self) -> &BTreeMap<u32, ClipObject> {
        &self.objects
    }

    /// The carried `/DR` font: the resource name its `/DA` uses, and the
    /// clip-local object holding the font dictionary.
    pub(crate) fn da_font_entry(&self) -> Option<(Vec<u8>, u32)> {
        self.da_font.clone()
    }
}

/// What a **cut** did: the clip that was carried, and the deletion that
/// made room for it (`Pass 168.0`).
///
/// Both halves are returned because both carry information the operator
/// needs and neither can be derived from the other. The clip says what is now
/// on the clipboard; the deletion says what leaving cost — a cleared
/// selection value, a pruned grouping node, how many widgets went.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct FieldCut {
    /// The field, on the clipboard. Serialise it with
    /// [`FieldClip::to_bytes`] to carry it to another document.
    pub clip: FieldClip,
    /// What removing it did to the form it left, including the disclosures
    /// `delete_field` owes: a `/V` that pointed at a state no remaining
    /// widget could show, and grouping nodes pruned because they became
    /// childless.
    pub deletion: crate::edit::FieldDeletion,
}

/// What a paste did, and everything about it the operator must be told.
///
/// # Why sentences rather than a struct of booleans
///
/// [`FieldAuthorDisclosures`](crate::edit::FieldAuthorDisclosures) is a fixed
/// set of flags because the things field *creation* can surprise you with are
/// a closed set: pdfcer chose every value, so it knows in advance what it might
/// have to say.
///
/// A paste's disclosures are not closed. What a clip carries — and therefore
/// what could be dropped, renamed, translated or degraded — depends on the
/// document it came from. A boolean per possibility would either be a struct
/// that grows on every real-world file, or a `dropped_something: bool` that
/// tells the operator nothing actionable. So this returns the sentences.
///
/// They are **off-canvas** disclosures in the sense of `CLAUDE.md` rule 4 as
/// narrowed by decision 059: the pasted field renders exactly as a
/// saved-and-reopened one will, with nothing drawn on the page to mark it as
/// pdfcer's guess, and these strings belong in a status line or a report.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FieldPasteOutcome {
    /// The field the paste landed in — a fresh one for
    /// [`FieldPastePolicy::NewField`], the existing one for
    /// [`FieldPastePolicy::AdditionalWidget`].
    pub field_id: ObjId,
    /// The widget annotation(s) now on the page, in placement order.
    ///
    /// For a merged (Shape A) single-widget paste this holds
    /// [`Self::field_id`] itself, because §12.5.6.19 lets one dictionary be
    /// both — the same shape every `add_*_field` verb writes.
    pub widget_ids: Vec<ObjId>,
    /// Whether the new field is the merged Shape A form.
    pub merged: bool,
    /// Whether a NEW field was created (`Ctrl+V`) rather than a widget added
    /// to an existing one (`Ctrl+Shift+V`).
    ///
    /// The one fact a shell needs to phrase its own confirmation correctly:
    /// *"a new field"* and *"another copy of the same field"* are different
    /// sentences, and getting them the wrong way round is exactly the
    /// confusion the two chords exist to remove.
    pub created: bool,
    /// Everything the operator must be told, in plain sentences.
    pub disclosures: Vec<String>,
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

    fn sample() -> FieldClip {
        let mut field = Dict::new();
        field.insert(Name::from(b"FT"), Object::Name(Name::from(b"Tx")));
        field.insert(Name::from(b"Ff"), Object::Integer(4096));
        field.insert(
            Name::from(b"DA"),
            Object::String(b"/Helv 14 Tf 0 0 1 rg".to_vec()),
        );
        field.insert(Name::from(b"Q"), Object::Integer(1));
        field.insert(Name::from(b"TU"), Object::String(b"Revision".to_vec()));

        let mut widget = Dict::new();
        widget.insert(Name::from(b"F"), Object::Integer(4));
        widget.insert(Name::from(b"AP"), Object::Reference(ObjId::new(1, 0)));

        let mut objects = BTreeMap::new();
        objects.insert(
            1,
            ClipObject {
                value: Object::Stream(Stream {
                    dict: Dict::new(),
                    data_span: ByteSpan::new(0, 5),
                }),
                payload: Some(b"hello".to_vec()),
            },
        );

        FieldClip {
            version: FIELD_CLIP_VERSION,
            field,
            widgets: vec![FieldClipWidget {
                dict: widget,
                rect: Rect {
                    llx: 10.0,
                    lly: 20.0,
                    urx: 110.0,
                    ury: 44.0,
                },
            }],
            objects,
            da_font: Some((b"Helv".to_vec(), 1)),
            source_name: "TitleBlock.Revision".to_owned(),
            field_type: Some(FieldType::Text),
            button_kind: None,
        }
    }

    /// The headline guarantee: nothing is lost across the wire.
    ///
    /// `ObjectClip` cannot say this — it drops its annotations. A field clip
    /// can, and the difference is that a field clip is dictionaries and
    /// streams rather than rich Rust enums.
    #[test]
    fn a_clip_round_trips_through_bytes_unchanged() {
        let clip = sample();
        let back = FieldClip::from_bytes(&clip.to_bytes()).expect("round trip");
        assert_eq!(back, clip, "every field survives serialisation");
    }

    #[test]
    fn an_unrelated_payload_is_refused_by_the_magic_not_by_a_length_prefix() {
        assert_eq!(
            FieldClip::from_bytes(b"not a clip at all, really"),
            Err(ClipError::NotAClip),
        );
        // An OBJECT clip is the near miss that matters: same project, same
        // shell, same clipboard.
        let object_clip = crate::vector::ObjectClip {
            version: 1,
            items: Vec::new(),
            objects: BTreeMap::new(),
            bbox: crate::vector::Bounds::EMPTY,
            annotations: Vec::new(),
        }
        .to_bytes();
        assert_eq!(
            FieldClip::from_bytes(&object_clip),
            Err(ClipError::NotAClip),
            "the two pdfcer clipboard formats must not be confusable",
        );
    }

    #[test]
    fn a_newer_format_is_refused_rather_than_half_understood() {
        let mut bytes = sample().to_bytes();
        bytes[12..16].copy_from_slice(&(FIELD_CLIP_VERSION + 1).to_le_bytes());
        assert_eq!(
            FieldClip::from_bytes(&bytes),
            Err(ClipError::NewerFormat {
                found: FIELD_CLIP_VERSION + 1,
                supported: FIELD_CLIP_VERSION,
            }),
        );
    }

    #[test]
    fn every_truncation_of_a_valid_payload_is_refused_without_panicking() {
        let bytes = sample().to_bytes();
        for cut in 0..bytes.len() {
            // The only requirement is that it does not panic; which refusal
            // it picks depends on where the cut lands.
            let _ = FieldClip::from_bytes(&bytes[..cut]);
        }
    }

    /// A hostile count must be refused BEFORE it is allocated for.
    #[test]
    fn an_absurd_widget_count_is_refused_by_the_ceiling() {
        let clip = sample();
        let mut bytes = clip.to_bytes();
        // Find the widget count: magic(12) + version(4) + name(4+len) + 2 tags
        // + field object(4 + len).
        let name_len = clip.source_name.len();
        let mut at = 12 + 4 + 4 + name_len + 2;
        let obj_len =
            u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as usize;
        at += 4 + obj_len;
        bytes[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            FieldClip::from_bytes(&bytes),
            Err(ClipError::ClipTooLarge {
                found: u32::MAX as usize,
                limit: MAX_CLIP_WIDGETS,
            }),
        );
    }

    #[test]
    fn the_accessors_answer_what_a_paste_ui_asks() {
        let clip = sample();
        assert_eq!(clip.source_name(), "TitleBlock.Revision");
        assert_eq!(clip.field_type(), Some(FieldType::Text));
        assert_eq!(clip.widget_count(), 1);
        assert!(!clip.carries_actions());
        assert!(!clip.carries_calculation());
        assert!(!clip.carries_value());
        assert_eq!(clip.tooltip(), Some(b"Revision".as_slice()));
        assert_eq!(clip.carried_font(), Some(b"Helv".as_slice()));
        assert_eq!(
            clip.bbox(),
            Some(Rect {
                llx: 10.0,
                lly: 20.0,
                urx: 110.0,
                ury: 44.0
            })
        );
    }

    /// The two chords must not be able to become each other by accident.
    #[test]
    fn a_policy_names_its_target_whichever_branch_it_is() {
        let new = FieldPastePolicy::NewField {
            name: "Rev2".to_owned(),
            tooltip: PasteTooltip::Carry,
            copy_value: false,
            copy_actions: true,
        };
        let widget = FieldPastePolicy::AdditionalWidget {
            existing: "Rev".to_owned(),
        };
        assert_eq!(new.target_name(), "Rev2");
        assert_eq!(widget.target_name(), "Rev");
        assert_ne!(new, widget);
    }
}
