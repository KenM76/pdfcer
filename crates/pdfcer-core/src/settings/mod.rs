//! Persisted operator settings — the R15 user-state partition.
//!
//! # Why this module exists, and why it exists *now*
//!
//! Two standing obligations converge here, one old and one new.
//!
//! **R15 (decision 003 §5.6, §6.1)** says the distribution folder is
//! partitioned from the start: *"Replaceable payload and user state are
//! separate; user state never sits loose among the binaries; the
//! documented update procedure names exactly which files to keep. Binding
//! from the first Pass that persists anything."* pdfcer's update story is
//! **manual replace-the-folder**, and replacing a folder destroys
//! everything in it — so the moment pdfcer writes a settings file into the
//! program directory, a routine update silently wipes the operator's
//! configuration. Decision 003 required this decided *before* the first
//! Pass that persists anything, precisely so it would never need
//! retrofitting onto existing users' state.
//!
//! **The 2026-08-08 operator directive** is what finally made something
//! need persisting: *"where standards are ambiguous those should become
//! settings that the user can choose direction one, with the initial
//! installed default as the best guess of what is usually followed."* The
//! spec RAG's ambiguity register triages **18** such settings out of 155
//! recorded findings, and **10 of the 18 are already hard-coded in shipped
//! source** — pdfcer has silently picked a side ten times. A setting that
//! forgets itself at restart is worse than no setting, so the store comes
//! first.
//!
//! It is also the shared prerequisite for three *other* operator asks that
//! have nothing to do with the spec: a fully customizable ribbon with
//! saveable layout configurations, mouse/keyboard bindings with saveable
//! configurations, and dock-layout persistence (`crate::` has no view of
//! that last one — see `pdfce-gui`'s `dock.rs`, which states outright that
//! nothing is written to disk and that serializing the dock tree is the
//! natural mechanism *"when R15 lands"*). Four asks, one missing
//! component.
//!
//! # Where the file lives
//!
//! `<directory of the running executable>/userdata/settings.txt`.
//!
//! Decision 003 wrote the folder as a literal `<user-state>` placeholder
//! and never named it — the README sentence it drafted reads *"replace the
//! program files (keep your `<user-state>` folder)"*. **`userdata` is that
//! name**, chosen here because it reads correctly in exactly that
//! sentence, is self-describing to someone who has never read the docs,
//! and is the convention portable Windows applications already use.
//!
//! ## The read-only-install fallback, and why it is disclosed
//!
//! `ARCHITECTURE.md` §6 requires pdfcer to *"run read-only-folder-clean"* —
//! an operator may put the program on a read-only share or in
//! `Program Files`. So when `userdata/` cannot be created or written,
//! [`resolve_store`] falls back to the platform configuration directory
//! and **says which one it used** ([`StoreLocation::kind`]).
//!
//! The disclosure is not decoration. The two locations behave differently
//! on update — the portable one is the operator's to preserve, the
//! platform one survives a folder replace by itself — so an operator who
//! does not know which one is live cannot follow the update instructions
//! correctly. This is the fuzzy-never-sneaky rule applied to a decision
//! pdfcer made on the operator's behalf: pdfcer inferred a location, so the
//! inference is visible.
//!
//! # The format, and why it is not TOML or JSON
//!
//! A flat, line-oriented `key = value` text file with `#` comments.
//!
//! The obvious move is `serde` plus `toml`. It was rejected, and the
//! reason is a requirement rather than a preference: **§7's fail-soft
//! contract is per-key, and derived deserialization is per-document.** A
//! `serde` derive presented with one unknown key, one misspelled enum
//! variant, or one out-of-range number fails the *whole* file, which would
//! discard every setting the operator got right because of the one they
//! got wrong — on a file they are explicitly invited to hand-edit. Writing
//! per-field recovery on top of `serde` means fighting it with
//! `#[serde(default)]` on every field plus a custom deserializer per
//! enum, which is more code than the twenty-line grammar below and still
//! cannot report *which line* was wrong.
//!
//! The grammar is small enough to state completely:
//!
//! ```text
//! line    := comment | blank | entry
//! comment := ws* '#' .*
//! blank   := ws*
//! entry   := ws* key ws* '=' ws* value ws*
//! key     := [A-Za-z0-9_.]+
//! value   := .*            (trailing whitespace trimmed; not unquoted)
//! ```
//!
//! No sections, no nesting, no escapes, no quoting. Values that would need
//! any of those do not belong in this file — a ribbon layout or a keymap
//! is a *document*, not a setting, and gets its own file under the same
//! `userdata/` roof rather than being crammed into this grammar.
//!
//! # The fail-soft contract (§7, A.6, R82)
//!
//! Nothing in this module returns an error to a caller who merely wants to
//! know what the settings are. [`Settings::load`] **always** produces
//! usable settings. Every departure from the file's literal content is
//! recorded as a [`SettingNote`] the front end can show:
//!
//! | Situation | Result |
//! |---|---|
//! | No file at all | Every default. **No note** — a first run is not a fault. |
//! | Directory unreachable / unreadable file | Every default, [`SettingNote::Unreadable`]. |
//! | Unknown key | Other keys still applied, [`SettingNote::UnknownKey`]. |
//! | Unparseable value | That key defaults, [`SettingNote::BadValue`] naming the value used. |
//! | Value out of range | Clamped, [`SettingNote::Clamped`]. |
//! | Malformed line (no `=`) | Skipped, [`SettingNote::Malformed`]. |
//! | Duplicate key | **Last wins**, [`SettingNote::Duplicate`]. |
//!
//! A missing file is deliberately silent and everything else is not. The
//! distinction is whether the operator did something: a first run is the
//! expected state, whereas a typo in a file they edited is a thing they
//! want told about *at the line number*, not discovered later by noticing
//! pdfcer behaves oddly.
//!
//! **Never an error dialog, never a lost document session** — `dock.rs`'s
//! wording, and it binds here because this store is what `dock.rs` was
//! waiting for. A configuration problem must not be able to stop pdfcer
//! opening a file.
//!
//! # What this module does not do
//!
//! - **It does not decide defaults.** Each default lives with the type it
//!   belongs to ([`Default`] impls elsewhere in the crate), so there is
//!   exactly one answer to "what does pdfcer do by default?" and it is not
//!   in a settings file. This module reads and writes; it does not define.
//! - **It does not watch the file.** Settings are read when asked. A file
//!   watcher would make the live configuration depend on when an editor
//!   happened to flush, which is a source of irreproducible behaviour, not
//!   a feature.
//! - **It does not write on exit.** [`Settings::save`] is called
//!   deliberately, so a crash cannot persist half a session's accidental
//!   state, and an operator's hand-edited file is never rewritten behind
//!   their back with pdfcer's own formatting.

/// Named render-setting bundles for the PDF subset standards
/// (PDF/X, PDF/A, PDF/UA) - each value carrying its own evidence tier.
pub mod presets;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::pageops::separation::SeparationPolicy;

/// File name of the settings file inside the user-state directory.
pub const SETTINGS_FILE: &str = "settings.txt";

/// Name of the user-state directory beside the executable.
///
/// The name decision 003 left as a `<user-state>` placeholder. It appears
/// verbatim in the update instructions, so changing it is a documentation
/// change and a migration, not a rename.
pub const USER_STATE_DIR: &str = "userdata";

/// Which of the two possible homes the settings file is actually using.
///
/// Surfaced rather than hidden because the operator's update procedure
/// differs between them — see the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StoreKind {
    /// `<exe dir>/userdata/` — the intended, portable location. The
    /// operator keeps this folder across an update.
    Portable,
    /// The platform configuration directory, used because the portable
    /// location was not writable (a read-only share, `Program Files`
    /// without elevation). Survives a folder replace on its own, and is
    /// **not** portable — it does not travel with the program folder.
    PlatformFallback,
    /// No writable location was found at all. Settings still load from
    /// defaults and the session works; saving will report why it cannot.
    None,
}

/// Where the settings file is, and how that was decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreLocation {
    /// The settings file's full path, absent only for [`StoreKind::None`].
    pub path: Option<PathBuf>,
    /// Which home this is.
    pub kind: StoreKind,
}

impl StoreLocation {
    /// The directory holding the settings file, if there is one.
    #[must_use]
    pub fn directory(&self) -> Option<&Path> {
        self.path.as_deref().and_then(Path::parent)
    }
}

/// One thing that happened while loading which the operator may want to
/// know about.
///
/// Every variant names the **line** where possible, because the whole
/// point of a hand-editable file is that a mistake in it is findable. A
/// note that says "a value was wrong" without saying which line is a note
/// that makes the operator re-read the entire file.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SettingNote {
    /// The file exists but could not be read.
    Unreadable {
        /// The path that failed.
        path: PathBuf,
        /// The operating system's reason, already rendered.
        reason: String,
    },
    /// A key pdfcer does not recognise. Left alone, never deleted — it may
    /// belong to a newer version the operator also runs from the same
    /// folder.
    UnknownKey {
        /// The key as written.
        key: String,
        /// 1-based line number.
        line: usize,
    },
    /// A known key whose value could not be interpreted.
    BadValue {
        /// The key.
        key: String,
        /// The value as written.
        value: String,
        /// 1-based line number.
        line: usize,
        /// What pdfcer used instead, already rendered.
        using: String,
    },
    /// A numeric value outside the range the setting accepts.
    Clamped {
        /// The key.
        key: String,
        /// The value as written.
        value: String,
        /// 1-based line number.
        line: usize,
        /// The value actually used, already rendered.
        using: String,
    },
    /// A line that is neither blank, a comment, nor `key = value`.
    Malformed {
        /// 1-based line number.
        line: usize,
    },
    /// The same key set more than once. The last occurrence wins, which
    /// is the behaviour that makes appending to the file work.
    Duplicate {
        /// The key.
        key: String,
        /// 1-based line number of the occurrence that won.
        line: usize,
    },
}

/// Everything [`Settings::load`] wants to tell the caller.
///
/// Separate from [`Settings`] so that the settings themselves stay a plain
/// value with no diagnostic baggage: a caller that only wants to know what
/// the operator chose does not carry the story of how it was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadReport {
    /// Where the file was looked for, and which home that is.
    pub location: StoreLocation,
    /// Whether a file was actually found.
    ///
    /// `false` on a first run, which is **not** a fault and produces no
    /// note.
    pub existed: bool,
    /// Everything worth telling the operator, in file order.
    pub notes: Vec<SettingNote>,
}

impl LoadReport {
    /// Whether anything at all needs saying.
    #[must_use]
    pub fn is_quiet(&self) -> bool {
        self.notes.is_empty()
    }
}

/// How `DeviceCMYK` is converted for display (ISO 32000-1 §8.6.4.4).
///
/// §8.6.4.4 mandates **no** conversion at all — it is device-dependent by
/// definition — so there is no correct answer to appeal to, and Acrobat's
/// own answer is a user-configurable working-space profile. That makes
/// this the textbook case for R169: the standard is silent, so the choice
/// is the operator's.
///
/// # ★★ THE DEFAULT, AS DATA
///
/// ```text
///   shipped default : Calibrated       (operator ruling, 2026-08-28)
///   best-evidenced  : Calibrated       (Acrobat's shipped profile + pdfium)
///   they differ     : NO. They agree.
/// ```
///
/// # ★★★ THEY DID NOT ALWAYS AGREE, AND THE HISTORY IS LOAD-BEARING
///
/// From 2026-08-08 to 2026-08-28 the shipped default was
/// [`Self::NeutralBlack`], by Ken's explicit ruling ("flip it") once he saw
/// what the calibrated answer does to pure-K line art. The evidence favoured
/// `Calibrated` throughout — tier (a)/(c), the strongest in the ambiguity
/// register — and lost to a deliberate judgement about CAD drawings.
///
/// **He reversed it on 2026-08-28**, relayed through `pdfcer-gui`, verbatim:
/// *"under the colour setting we are going to change our default to Match
/// other PDF viewers."*
///
/// ★ The reversal is recorded rather than erased for one specific reason:
/// **`NeutralBlack`'s reasoning did not stop being true.** Pure-K line art
/// still renders `#231F20` under the new default, and that is still not what
/// a CAD operator expects. What changed is which of two good answers ships
/// first. A future session finding a drawing's blacks "wrong" should reach
/// for [`Self::NeutralBlack`], not treat it as a defect.
///
/// # ★★ A NOTE THAT DIED WITH ITS DIVERGENCE, AND WHY THAT MATTERED
///
/// This block previously carried a long argument that pdfcer's default
/// *knowingly diverged* from Acrobat, written after `pdfce-gui` misread the
/// prose and restated `NeutralBlack` in its own PDF/X preset believing it was
/// diverging from pdfcer — it never was.
///
/// With the default now matching, that argument is not merely redundant, it
/// is **backwards**: a note explaining a divergence, left standing after the
/// divergence ends, actively misinforms. `pdfcer-gui` made the same call on
/// their side and deleted their divergence note rather than rewording it.
///
/// ★ The generalisable half survives the deletion and is kept here because
/// it outlived its example: **a doc comment that describes a default as a
/// divergence invites being read as a divergence in VALUE.** State the
/// shipped default and the best-evidenced answer as two lines of data. When
/// they agree, say so — an absent statement reads as an unexamined one.
///
/// # A variant was DELETED here, which is rarer than a default moving
///
/// What pdfcer does when the operator asks for **bold or italic** and the
/// ideal face may or may not be there (`Pass 179.0`, decision 106).
///
/// # ★ The operator ruled this, twice, and the second half is why it is a
/// setting rather than a constant
///
/// First (2026-08-30): *"bold font should be automatically used if available,
/// but otherwise synthetic should be supported, and the user shouldn't have to
/// intervene."* That gives [`Self::Auto`] and makes it the default.
///
/// Then, immediately after: *"let's still make the current method of warning
/// or forcing it manually or refusing available as well as the automatic
/// silent one."* So the postures pdfcer already had are **kept**, not replaced.
///
/// This is the same argument [`SeparationPolicy`] carries and the same reason:
/// **all three answers are defensible for different workflows.** A drawing
/// office wants the bold and does not want to be asked; a typographer setting
/// body text wants to be told when a weight was faked; a conformance run wants
/// to be stopped rather than given a fake. None of those is wrong.
///
/// # What each posture does NOT change
///
/// The **ladder** is the same in every posture — a real face is always
/// preferred to a synthesised one, and the rung order never varies. What
/// varies is only **what pdfcer does about a fallback once it has picked one**.
/// A posture that changed which face was chosen would be a second resolution
/// path, and two paths drift.
///
/// Naming the face explicitly (`set_font`) or forcing synthesis
/// (`set_synthetic`) is the operator "forcing it manually" and is honoured
/// under [`Self::Auto`] and [`Self::Warn`] exactly as asked — those controls
/// are unchanged and are not a posture.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum StylePolicy {
    /// **Decide and apply, silently. The default** (operator ruling,
    /// 2026-08-30).
    ///
    /// Walk the ladder, bind the best face available, synthesise only if
    /// nothing real is reachable, and never refuse and never ask. Which rung
    /// was used is **reported in the outcome** — rule 4 obliges disclosure,
    /// not a gate, and the operator's *"shouldn't have to intervene"* forbids
    /// the gate specifically.
    #[default]
    Auto,
    /// As [`Self::Auto`], but a **fallback is warned about**.
    ///
    /// The edit still happens — this is not a refusal and not a prompt. It
    /// raises the disclosure from a reported field to a warning a caller has
    /// to actively ignore, for a workflow where a faked weight in the output
    /// is a problem worth noticing at the moment it is created.
    ///
    /// A rung-1 or rung-2 result — a genuine face — warns about nothing,
    /// because nothing was faked.
    Warn,
    /// **Refuse a synthesis request when a real face is available**, naming
    /// the face — pdfcer's behaviour before `Pass 179.0`, kept because the
    /// operator asked for it to be kept.
    ///
    /// Strictly narrower than the others: it only ever fires on an **explicit**
    /// `set_synthetic` request that the ladder could have satisfied for real.
    /// An automatic bold request still resolves normally, and a run with no
    /// real face anywhere still synthesises — there is nothing to refuse in
    /// favour of.
    Refuse,
}

/// `CmykIntent::Naive` — the additive `1 − min(1, x + k)` formula pdfcer used
/// before it was calibrated — was removed by the same ruling: *"you can also
/// remove the old pdfcer formula from that section, even the code for it."*
///
/// It existed so an operator could reproduce a pre-calibration pdfcer export.
/// That justification was true when written and **expired silently as those
/// files aged out** — no test failed, no gate fired, and the copy describing
/// it still read sensibly. A control whose only purpose is bug-compatibility
/// with your own past is removable only by somebody deciding to, because
/// nothing will ever tell you it has become dead weight.
///
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum CmykIntent {
    /// The calibrated table in [`crate::color`] — agreement with the
    /// SWOP-family rendering that Acrobat's default profile and pdfium
    /// both produce.
    ///
    /// **The shipped default** (`Pass 153.0`, operator ruling 2026-08-28),
    /// and also the best-evidenced answer — the two agree now, and the type
    /// docs record that they did not always.
    ///
    /// Its visible consequence is that solid black ink (`0 0 0 1 k`) renders
    /// `#231F20` rather than `#000000`, and mid greys come out slightly cool.
    ///
    /// ★ **`#231F20` IS ONE OF TWO ANSWERS, ONE COUNT APART, AND BOTH ARE
    /// CORRECT.** Measured 2026-08-29 with the ink probe (`Pass 174.0`) on one
    /// page rendered both ways:
    ///
    /// ```text
    /// 0 0 0 1 k, page composited on screen      35, 31, 31   #231F1F
    /// 0 0 0 1 k, page composited in the buffer  35, 31, 32   #231F20
    /// ```
    ///
    /// The two paths reach this same table at different precisions — one
    /// converts an 8-bit paint colour, the other converts `f32` colorants at
    /// the very end (`CmykBuffer::to_srgb_over_white`) — so **every**
    /// `DeviceCMYK` colour carries a ±1 blue uncertainty that is a property of
    /// the compositing path and not of the table. The saturated green this
    /// project has chased all week shows exactly the same thing: `47,180,73`
    /// on screen against `47,181,73` in ink.
    ///
    /// Recorded because it is the kind of one-count difference a future
    /// session will find, read as a defect in one of the two paths, and try to
    /// remove. It is not a defect. Every doc comment in this workspace that
    /// states `#231F20` is quoting the colorant path and is right to — a
    /// `grep` will find them, and a count stated here would be one more number
    /// nothing keeps true.
    ///
    /// # ★★ THE SECOND HALF OF THAT SENTENCE WAS A CLAIM, AND IT HAS BEEN
    /// MEASURED. It was wrong.
    ///
    /// This paragraph read, until `Pass 174.1`:
    ///
    /// > ~~"…and mid greys are slightly cool. **That is what Acrobat shows,
    /// > which is the point**: *"what will this look like in Acrobat?"* and
    /// > *"what does pdfcer show?"* now have one answer."~~
    ///
    /// Measured 2026-08-29 against a reference engine's own renders of the
    /// licensed print-conformance corpus, over **every flat achromatic region
    /// of at least 2 000 px in all 51 patches** (`tools/flat-color-parity.py
    /// --neutrals`), with the probe (`--probe-ink`) confirming each grey's
    /// route:
    ///
    /// ```text
    ///   pure-K ink      reference        pdfcer (Calibrated)     spread
    ///   0 0 0 0.500     156,156,156       147,149,152             5
    ///   0 0 0 0.749      98, 98, 98        99,100,103             4
    /// ```
    ///
    /// **The reference renders pure-K greys EXACTLY neutral — channel spread
    /// zero — at both levels the corpus offers. pdfcer does not, at either.**
    /// (Two, not more: a third achromatic pair, on the output-intent-change
    /// patch, is excluded because pdfcer's whole region there is 65–70 counts
    /// dark, which is a different defect and not a hue one. Two levels is a
    /// small sample and is stated as one.)
    /// So the cool cast is a divergence, not agreement, and the sentence that
    /// justified it was justifying the opposite of what it claimed.
    ///
    /// ★ **The other half of the claim SURVIVES, and separating them is the
    /// point of stating both.** `Calibrated` tracks the reference's
    /// *lightness* closely — `99` against `98` at `K = 0.749` — where a naive
    /// `1 − K` formula would have given `64`, more than thirty counts out.
    /// The table is right about how dark ink looks and wrong about its hue.
    /// A future session must not read this note as an argument for reverting
    /// to a naive conversion; it is an argument about **neutrality**, on the
    /// achromatic axis only.
    ///
    /// ★★ **The aggregate number is the trap here, and it is recorded so the
    /// measurement is not re-run and re-misread.** Over the same corpus,
    /// **125 of 132** achromatic reference regions come out of pdfcer still
    /// achromatic — 95 %, which reads as a strong result and is one. Segment
    /// out the regions the reference painted **paper white**, where a
    /// conversion that did nothing at all would also score perfectly, and the
    /// population is **7 mid-grey regions of which pdfcer leaves 0 neutral**.
    /// A fixture whose expected value equals what the code writes anyway
    /// cannot falsify anything, and 125 of those 132 were that fixture.
    ///
    /// # ★★ A THIRD INDEPENDENT LINE ARRIVED, AND IT MOVES THE "THIN BASIS"
    /// SENTENCE BELOW
    ///
    /// The paragraph after this one says two grey levels is a thin basis for
    /// moving a shipped default. That was true when written and is weaker now,
    /// so it is qualified here rather than left to read as current.
    ///
    /// On 2026-08-29 the sibling `iccce` project ran the **actual ICC
    /// transform** — the print-conformance patch's own `/DestOutputProfile`
    /// (a v2.4.0 `prtr` CMYK/Lab) to the OS-shipped sRGB profile,
    /// media-relative colorimetric — over 49 operands, and corroborated its
    /// own arithmetic against `lcms2` 2.19.1 to **0.22 counts**. On the
    /// achromatic axis:
    ///
    /// ```text
    ///   pure-K ink   reference    pdfcer (Calibrated)   iccce (real transform)
    ///   0 0 0 0.50   156,156,156     147,148,152           158,159,159
    ///   0 0 0 0.35        —          177,178,182           189,189,190
    /// ```
    ///
    /// **iccce returns NEUTRAL greys and lands within 2–3 counts of the
    /// reference; pdfcer is cool on every row, blue above red by 2 to 5.** So
    /// the evidence for the hue divergence is now three lines that did not
    /// come from each other: the reference's own exact neutrality, pdfcer's
    /// measured spread over the corpus, and the profile's own answer computed
    /// by a separate engine against `lcms2`. **Still not a reason to change
    /// the conversion here** — decision 064 puts it in `iccce`'s domain and
    /// the operator ruled the default — but a future session weighing the
    /// evidence should weigh three lines, not two.
    ///
    /// # ★★★ AND THE BLACK END IS A FALSE-DEFECT TRAP — pdfcer IS THE CLOSER
    /// ANSWER THERE, WHICH THE TABLE DOES NOT SHOW
    ///
    /// The same 49-operand comparison disagrees far more at the dark end
    /// (median worst-channel difference 11 counts, maximum 34), and **on every
    /// one of the six worst rows `iccce` is LIGHTER**:
    ///
    /// ```text
    ///   operand              pdfcer        iccce
    ///   0.00 0.00 0.00 1.00  35, 31, 31   43, 43, 42
    ///   1.00 1.00 1.00 1.00   0,  0,  0   28, 27, 24
    ///   0.10 1.00 1.00 1.00  11,  0,  0   45, 28, 22
    /// ```
    ///
    /// **Read naively, that table says pdfcer's blacks are 8 to 34 counts
    /// wrong.** They are not, and `iccce` said so unprompted and in its own
    /// words: *"on the black end, do not read my column as the better answer …
    /// it is the answer of an engine that declined to estimate something yours
    /// effectively assumes. Refusing and being wrong look identical in a
    /// table; only this paragraph distinguishes them."*
    ///
    /// The mechanism is **black point compensation**. Media-relative *without*
    /// BPC returns the profile's actual darkest printable colour, which is
    /// what `iccce` returns because its black-point estimator **refuses by
    /// name** on this profile rather than guessing. Acrobat's display path is
    /// almost certainly media-relative *with* BPC, which pulls that black down
    /// toward display black — and pdfcer's `35, 31, 31` is within a count of
    /// the `#231F20` this type documents for K-only black, i.e. pdfcer matches
    /// the reference here and `iccce` does not.
    ///
    /// ★ **Recorded because the trap is asymmetric and invisible in the
    /// numbers.** The SAME comparison makes pdfcer look wrong on the grey axis
    /// (it is) and wrong on the black end (it is not), and nothing in the
    /// table distinguishes the two. A session that "fixes" the black end
    /// toward `iccce`'s column would move pdfcer AWAY from the reference by up
    /// to 34 counts while believing it had closed a gap. *"Which engine is
    /// better"* is the wrong question — different regions, different answers,
    /// different causes.
    ///
    /// ★ **Not a fitting target either way.** `iccce`'s own position, held to
    /// symmetrically: *"you should not hand-tune toward these 49 numbers any
    /// more than I should tune toward the Acrobat capture."* 49 pixels bought
    /// at the cost of every other one is the trade decision 064 exists to
    /// prevent. The right use is a **regression datum** — what a real
    /// transform through a real declared output condition returns for the
    /// operands pdfcer actually paints.
    ///
    /// **Not changed here, deliberately.** The conversion itself is the
    /// sibling `iccce` project's domain under decision 064, and the operator
    /// ruled this default on 2026-08-28. What is owed is an accurate
    /// justification, and that is what this is. (The "two levels is a thin
    /// basis" reasoning above stands as the state of the evidence when it was
    /// measured; the third line qualifying it is recorded two sections up
    /// rather than by rewriting it, because the sequence is the part a future
    /// session needs.)
    #[default]
    Calibrated,
    /// As [`Self::Calibrated`], except that pure black — `C = M = Y = 0`
    /// with any `K` — is forced to a neutral grey of `1 − K`, so pure-K
    /// line art renders `#000000`.
    ///
    /// **Was the shipped default until 2026-08-28**, by an operator ruling
    /// the same operator has since reversed. Still the right choice for CAD
    /// and engineering drawings, where every line is stroked in pure K and
    /// true black on white is the expectation — the reasoning did not stop
    /// being true, it stopped being the default.
    NeutralBlack,
}

/// Where a page's blending colour space comes from when the page group
/// does **not** declare one (spec ambiguity `PGB-A1`).
///
/// # The silence being filled, and why it is edition-dependent
///
/// **ISO 32000-1 is determinate, and it is determinate AGAINST consulting
/// the output intent.** §11.4.7 and §11.6.3 each state it independently:
/// *"If not otherwise specified, the page group's colour space **shall**
/// be inherited from the native colour space of the output device."*
/// `shall`, no hedge. And `/OutputIntent` is **absent from the 1.7
/// transparency model entirely** — the spec corpus records this as a
/// *measured* negative (`PGB-N1`), not an unfound one: a proximity scan of
/// every "output intent" line in the 756-page source against
/// `blend`/`composit`/`transparen` returns exactly one hit, §8.6.5.5's ICC
/// sentence, inspected and excluded. §14.11.5's *"informational purposes
/// only … free to disregard"* survives verbatim into 2.0.
///
/// **ISO 32000-2 opens it, and only informatively.** §11.4.7 inherits from
/// the *"actual, **assumed or simulated**"* output device and says a
/// processor **can** choose which; **Annex P is informative** and offers
/// *"from the output device, **or** from the output intent"* with **no
/// ranking, no condition and no precedence**; §11.4.7 NOTE 3 names PDF/X-4's
/// output intent as the *"implied default page blending colour space"*. The
/// only body-text rung is §10.8.3(a), a **`should`**, and it selects a
/// *colourant set* rather than a blending space.
///
/// ⇒ Two conformant PDF 2.0 processors render the same file in two
/// different blending spaces and both cite Annex P. That is what makes this
/// a setting rather than a bug: there is no reading of the standard under
/// which one of these answers is simply wrong.
///
/// # What turns on it
///
/// Overprint. §8.6.7 *prescribes* the additive branch — *"source colours
/// **shall** be converted to the device's native colour space, and **all
/// components participate in the conversion, whatever their values**"* — so
/// [`Self::DeviceNative`] is **conforming but degenerate**, never
/// "unspecified".
///
/// ★ And the degeneracy is **structural, not approximate**. §11.7.4.3's
/// second bullet makes `B(c_b, c_s)` equal `c_s` for every component
/// *"specified in the current colour space"*; in sRGB every source colour
/// has already been converted to all three components, so every component
/// is specified and `B = c_s` **everywhere**. Overprint is therefore not
/// merely unsimulated in an additive space — it is **unrepresentable**, and
/// no amount of compositing work recovers it. Only an n-colorant buffer
/// does. This is worth knowing before anyone attempts a cheaper fix.
///
/// Measured on the print-conformance suite: **24 of its 51 patches request
/// overprint and receive no colorant buffer** under [`Self::DeviceNative`],
/// and those 24 contain every remaining failure in the suite.
///
/// # Disclosure
///
/// Whichever source is used, the resulting blending space and **its
/// provenance** are reported off-canvas — `pdfcer` prints them on the
/// metrics line, and nothing is drawn on the page (project rule 4). An
/// inferred blending space is exactly the kind of invisible inference that
/// rule exists for: it changes every colour on the page and leaves no mark
/// saying so.
/// Which colour spaces get `OPM 1`'s zero-tint rule under overprint
/// (`Pass 143.0`) — turned into a setting per the standing practice rather
/// than decided silently.
///
/// # ★★★ WHAT THIS IS, CORRECTED 2026-08-29 (`Pass 174.5`): IT IS A
/// DIVERGENCE UNDER ISO 32000-1, NOT A TWO-READINGS SILENCE
///
/// This block, and this type's own first line, said *"a genuine spec
/// ambiguity"* and *"there is no sentence resolving it either way"*. Audited
/// against the spec corpus by `pdfcer-spec-librarian` and **that is wrong for
/// ISO 32000-1, on three independent grounds** (register `OP-A5`):
///
/// 1. **§8.6.7's very next sentence excludes it in terms.** *"It shall not
///    apply … to any colours that are the result of a computation, such as
///    those in a shading pattern **or conversions from some other colour
///    space**."* A `DeviceGray` → `DeviceCMYK` map is precisely that, and
///    §10.3.3 even specifies the arithmetic as a `shall`.
/// 2. **Tables 148/149 row 2 enumerate the case and give it `OPM 0`
///    behaviour.** Source space *"Any process colour space (including other
///    cases of `DeviceCMYK`)"* × process colorant × `OP true, OPM 1` =
///    **"Paint source"**, identical to the `OPM 0` column. The standard did
///    not omit non-`DeviceCMYK` process spaces; it tabulated them.
/// 3. §8.6.7's escape hatch points at §8.6.5.7, **"Implicit Conversion of
///    CIE-Based Colour Spaces"** — CIE-based and nothing else. (This third
///    point is the only one the previous wording had, and alone it does read
///    like a silence.)
///
/// ★★ **ISO 32000-2 DELETES TWO OF THE THREE**, so the question is
/// **edition-gated**: 2.0 replaces the computed-colour sentence with a bare
/// *"images or shadings"*, and drops the opaque-model table entirely. Under
/// 2.0 this is much closer to a real silence. Under 1.7 it is not.
///
/// ⇒ **[`Self::GreyAsKOnly`], the shipped default, is a deliberate
/// divergence from ISO 32000-1**, and it must be *described* as a divergence.
///
/// ★ This sentence said "…divergence from ISO 32000-1 **toward Acrobat**, and
/// it is the right default". Both halves are now qualified. It diverges toward
/// Acrobat *over a spot backdrop* and **away** from Acrobat over process
/// components, measured 2026-09-01 — so "toward Acrobat" is not a property of
/// the setting, it is a property of the geometry you test it on. Neither value
/// matches Acrobat everywhere, because the real difference is that Acrobat has
/// a spot plane and pdfcer does not. See [`Self::GreyAsKOnly`] for the numbers.
/// A divergence owes the operator a disclosure that an ambiguity does not,
/// and calling it an ambiguity was quietly discharging that obligation by
/// misnaming it.
///
/// # ★ AND A TEST-DESIGN CONSEQUENCE THAT ALREADY COST A MEASUREMENT
///
/// Tables 148/149 also say: *"Any process colour space"* × **spot colorant**
/// × `OP true` = `c_b` — **"do not paint"** — in **both** the `OPM 0` and
/// `OPM 1` columns. So a grey fill over a **spot** backdrop preserves that
/// backdrop under **all three** of these settings and under **either**
/// reading.
///
/// **A grey-over-spot patch therefore cannot discriminate this setting at
/// all.** `Pass 174.2` ran exactly that ablation on the conformance corpus,
/// got bit-identical ink from all three values, and reported it as evidence
/// the setting was not the cause. The conclusion was right and the
/// *inference* was weaker than it looked: the result was forced by the table
/// and would have been identical on a correct implementation and a broken
/// one. **The discriminating case is grey over PROCESS components.**
/// Recorded as `OP-N3` in the spec register so nobody re-runs it.
///
/// # What the difference looks like on paper
///
/// A 50 % `DeviceGray` fill overprinting a spot backdrop. Under
/// [`Self::DeviceCmykOnly`] the grey paints all four components and **knocks
/// the spot out**; under [`Self::GreyAsKOnly`] its zero C, M and Y preserve
/// the backdrop and only K is laid down. Measured against Acrobat on the
/// print-conformance suite, **over a SPOT backdrop**: 84,120,34 (Acrobat, and
/// this setting's default) versus 127,127,127 (the literal reading).
///
/// ★ The qualifier "over a spot backdrop" was added 2026-09-01 and changes
/// what this measurement supports. It is a real measurement and it still
/// holds — but it was being read as "the default matches Acrobat", full stop,
/// and over PROCESS components the opposite is true (Acrobat 255,255,255,
/// this default 142,198,63). The agreement here is also for the wrong reason:
/// pdfcer flattens the spot into C/M/Y for want of a plane, and this reading's
/// mis-assignment then happens to preserve exactly those channels.
///
/// ★ **This example is about what PDFCER does, not about what the standard
/// requires**, and the difference is the section above. Tables 148/149 put
/// *"any process colour space" × spot colorant × `OP true`* at `c_b` under
/// **both** overprint modes — so a *conforming* engine preserves that spot
/// backdrop whichever way this setting is read. The reason pdfcer's two
/// settings differ here at all is that pdfcer **flattens a spot into C, M and
/// Y**, so there is no spot colorant for that table row to protect and the
/// paint meets the process-colorant row instead. The observable difference
/// is real; its cause is pdfcer's representation, and it will change when the
/// n-colorant buffer lands.
///
/// # Why no colour conversion is needed to implement it
///
/// The renderer already resolves a `DeviceGray` paint to CMYK before the
/// overprint rules see it — `overprint::rgb_to_cmyk` on equal RGB yields
/// `[0, 0, 0, 1-g]`, i.e. K-only, exactly. **Only the CLASSIFICATION
/// changes.** The spot backdrop's ink is likewise already in the four CMYK
/// planes by paint time (a `Separation` paint goes through its tint transform
/// into them), so *"preserve the spot backdrop"* is expressible with the
/// four-component rules alone: **this needs no new colorant plane and is not
/// blocked on the n-channel compositor.**
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum OverprintZeroTintScope {
    /// §8.6.7 to the letter: only a **direct `DeviceCMYK`** source gets
    /// `OPM 1`'s zero-tint rule. A `DeviceGray` or `DeviceRGB` paint with
    /// `/OP true` changes nothing about which components it writes.
    ///
    /// **Strictly conforming, and THE SHIPPED DEFAULT since `Pass 244.0`
    /// (2026-09-03).** It was the non-default from `Pass 143.0` to
    /// `Pass 243.0` for the sequencing reason [`Self::GreyAsKOnly`] records
    /// at length: flipping it before pdfcer had a per-spot-colorant plane
    /// traded one wrong cell for another. The plane landed in `Pass 238.0`/
    /// `239.0`, and with it in place this reading was re-measured on the
    /// print-conformance sweep: **0 FAIL / 43 pass of 51**, against
    /// 2 FAIL / 41 pass under [`Self::GreyAsKOnly`] — the two failures being
    /// exactly the two grey-over-process cells whose reference render this
    /// reading matches (`255,255,255` both). Three patches change a pixel
    /// under the flip: those two, and a font-support page whose grey text
    /// rows move TOWARD the reference. Nothing else moves.
    ///
    /// The sentence this paragraph replaces said "it knocks a spot backdrop
    /// out where Acrobat preserves it" — true only while pdfcer flattened
    /// spots into C/M/Y. With a spot plane, a grey over a spot backdrop is
    /// Table 149's "any process space × spot colorant × OP true = c_b" under
    /// EVERY value of this setting (`OP-N3`), so the spot survives here too.
    #[default]
    DeviceCmykOnly,
    /// Additionally treat a **`DeviceGray`** source as the K-only
    /// `DeviceCMYK` it converts to, so its zero C, M and Y preserve the
    /// backdrop.
    ///
    /// ★★ **The shipped default from `Pass 143.0` to `Pass 243.0`; NOT the
    /// default since `Pass 244.0`** — see [`Self::DeviceCmykOnly`] for the
    /// measurement that moved it. Kept as a selectable value: it is what
    /// every pdfcer release up to v0.24.0 rendered, and the setting exists
    /// precisely so a reading is chosen rather than hard-coded. The paragraphs
    /// below are the record of WHY it was the default and why that reasoning
    /// ran out; they are kept legible rather than rewritten.
    ///
    /// This was "the shipped default, and this is a print-conformance axis
    /// whose measurement instrument is authored to press behaviour — so the
    /// default is determined by what the instrument is for, not by a
    /// preference."
    ///
    /// # ★★ THE SENTENCE THAT USED TO END THAT PARAGRAPH IS FALSE
    ///
    /// It read: *"Acrobat does this; the suite is scored against Acrobat."*
    /// It is quoted rather than deleted because it is the entire stated
    /// justification for this being the default, and it was never checked
    /// against the one geometry that can check it.
    ///
    /// Over a **spot** backdrop — the only shape pdfcer had a fixture for —
    /// both readings produce a defensible picture and neither identifies what
    /// the reference engine does. The discriminating case is grey over
    /// **process** components, which this project's own note `OP-N3` had
    /// already named as the missing measurement.
    ///
    /// Measured 2026-09-01 on a conformance patch of exactly that shape (a
    /// `1 g` mark under `/OP true /OPM 1` over a `0.5 0 1 0 k` backdrop):
    ///
    /// | | result |
    /// |---|---|
    /// | this default | `142,198,63` — backdrop preserved |
    /// | [`Self::DeviceCmykOnly`] | `255,255,255` — backdrop replaced |
    /// | **Acrobat** | **`255,255,255`** |
    ///
    /// So the **literal** reading is the one that matches Acrobat, and this
    /// default does not. `1 g` converts to `0 0 0 0` under any profile, so
    /// "convert then apply OPM 1" cannot account for the difference — the row
    /// assignment can, and ISO 32000-1 §11.7.4.5 Table 149 places a
    /// `DeviceGray` source in row 2 (*"any process colour space"*, `c_s` in
    /// all three columns), not in row 1, whose scope note says `DeviceCMYK`.
    ///
    /// # Why the default was nevertheless UNCHANGED until `Pass 244.0`
    ///
    /// (Historical: the condition named below — the per-spot-colorant plane
    /// — was met in `Pass 238.0`/`239.0`, and the flip followed once it was
    /// re-measured.) A sequencing decision, not an endorsement. Flipping it alone was
    /// trap-neutral across the conformance corpus: it corrects one cell and
    /// breaks another that passes today only through a **compensating
    /// error** — pdfcer flattens a spot colorant into C/M/Y for want of a spot
    /// plane, and this wrong row assignment then happens to preserve exactly
    /// those planes. Two cells swap between near-zero and ~50 mean error
    /// while the page aggregate barely moves, which is precisely the shape a
    /// whole-page metric cannot see.
    ///
    /// The honest fix is the literal row assignment **together with** the
    /// per-spot-colorant plane. Changing this default before that plane
    /// exists would trade one wrong cell for another and call it progress.
    ///
    /// Scoped to `DeviceGray` and no wider **because that is the extent of
    /// what was measured**: of the suite's 16 `Separation`-plus-`/OP true`
    /// patches, **none** carries a `DeviceRGB` fill, so extending the rule to
    /// RGB here would be an unmeasured behavioural change riding along with
    /// a measured one. [`Self::AllProcessSpaces`] is where that lives, opt-in.
    GreyAsKOnly,
    /// Treat **every** process space as the `DeviceCMYK` it converts to —
    /// `DeviceRGB` and `CalRGB` as well as `DeviceGray`.
    ///
    /// The most principled reading of *convert-then-`OPM`*: if the argument
    /// works for grey it works for any space that resolves to CMYK tints.
    /// **Not the default, because it is unmeasured.** A `DeviceRGB` source
    /// generally produces non-zero C, M and Y, so this changes little in
    /// practice — but *"changes little"* is a prediction, and the suite
    /// contains no patch that would falsify it.
    ///
    /// ★ The specific hazard, stated so it is not discovered later: pdfcer's
    /// RGB→CMYK is a **naive** conversion, so a pure red `(1, 0, 0)` becomes
    /// `C = 0` and would preserve a cyan backdrop under this setting. Whether
    /// Acrobat agrees is not known here and was not measurable with the
    /// patches available.
    AllProcessSpaces,
}

impl OverprintZeroTintScope {
    /// Parse a settings-file / command-line token, or `None` if unknown.
    ///
    /// ★ ONE VOCABULARY, TWO READERS. The settings parser and
    /// `pdfcer render-page --overprint-zero-tint-scope` both come here, so
    /// a token the file accepts and a token the flag accepts cannot diverge.
    /// The alternative — a `match` in each — is two spellings of one enum,
    /// and the second one is always the one that goes stale.
    ///
    /// ```
    /// use pdfcer_core::settings::OverprintZeroTintScope as Scope;
    /// assert_eq!(Scope::parse("grey_as_k_only"), Some(Scope::GreyAsKOnly));
    /// assert_eq!(Scope::parse("device_cmyk_only"), Some(Scope::DeviceCmykOnly));
    /// assert_eq!(Scope::parse("nonsense"), None);
    /// ```
    #[must_use]
    pub fn parse(token: &str) -> Option<Self> {
        match token {
            "device_cmyk_only" => Some(Self::DeviceCmykOnly),
            "grey_as_k_only" => Some(Self::GreyAsKOnly),
            "all_process_spaces" => Some(Self::AllProcessSpaces),
            _ => None,
        }
    }

    /// The settings-file token for this value — the exact inverse of
    /// [`Self::parse`].
    ///
    /// ```
    /// use pdfcer_core::settings::OverprintZeroTintScope as Scope;
    /// for s in [Scope::DeviceCmykOnly, Scope::GreyAsKOnly, Scope::AllProcessSpaces] {
    ///     assert_eq!(Scope::parse(s.as_str()), Some(s), "round trip");
    /// }
    /// ```
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DeviceCmykOnly => "device_cmyk_only",
            Self::GreyAsKOnly => "grey_as_k_only",
            Self::AllProcessSpaces => "all_process_spaces",
        }
    }
}

/// Which **output-device model** pdfcer renders a spot colorant against
/// (`OP-A7`).
///
/// # ★★ This is a real fork in the standard, not a quality knob
///
/// ISO 32000-1 **§8.6.6.4** contains a `shall` that fires the moment a
/// `Separation` colour space is *set*, long before any overprint rule is
/// consulted:
///
/// > the conforming reader **shall determine whether the device has an
/// > available colorant** corresponding to the name of the requested space
/// > … **if it does not**, it *"shall arrange for subsequent painting
/// > operations to be performed in an alternate colour space."*
///
/// A screen has no `PANTONE 265 C` plate. Read literally, that clause says
/// a composite device must substitute the alternate space — after which the
/// ink is ordinary process colour and **overprint can no longer preserve
/// it, because there is no longer a spot colorant on the page to preserve.**
///
/// ISO 32000-**2** §10.8.3 then defines *separation simulation*: render as
/// if for a **simulated** device that does have the colorant. That is a
/// `may`, and it is 2.0-only — ISO 32000-1 has no such concept at all.
///
/// ⇒ **Both answers are conformant, and they render differently.** A white
/// object overprinting a spot backdrop knocks it out under one and preserves
/// it under the other. That is why this is a setting: the standard does not
/// choose, so pdfcer must not pretend it did.
///
/// # Why the default is [`Self::SimulateSeparations`]
///
/// Because §8.6.6.4's own **NOTE 7** says the alternate-space path *"does
/// not necessarily reflect the interactions between an object and its
/// backdrop when overprinting is enabled"* and points at separation
/// simulation as *"an alternative method to yield better results when
/// overprinting is involved"* — and §10.8.2's worked example (cyan then
/// yellow, overprinting) shows the two paths giving *"dramatically different
/// colours"*, with the composite one wrong.
///
/// ★ **Stated honestly, because the asymmetry is real:** the default has
/// **no ISO 32000-1 basis whatsoever**. It is the recommended branch of an
/// *optional* 2.0 feature. "On the recommended branch" is a weaker claim
/// than "the conforming one", and an earlier decision record made the
/// stronger one before this clause was found.
///
/// # ★ The consequence for any test that uses another engine as an oracle
///
/// A viewer in composite mode ([`Self::AlternateSpaceSubstitution`]) and one
/// simulating separations produce **different, both-correct** pixels for the
/// same file. So an expected-colour fixture for a spot-overprint cell
/// encodes an unstated assumption about which model the oracle was in, and
/// will fail correct code the moment that oracle's preference changes.
/// Expected values for such cells belong **per device model**.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum SpotColorantDeviceModel {
    /// Render for a **simulated device that has the colorant** — ISO
    /// 32000-2 §10.8.3. The spot keeps its own ink plane, and overprint
    /// preserves it.
    ///
    /// **The default.** See the type's docs for why, and for the honest
    /// caveat that this branch is 2.0-only and optional.
    #[default]
    SimulateSeparations,
    /// Render for the **actual composite device**, which has no such
    /// colorant — ISO 32000-1 §8.6.6.4's `shall`. The `Separation` is
    /// converted through its tint transform at the moment its space is set,
    /// and from then on it is ordinary process ink.
    ///
    /// **Choose this to reproduce a composite viewer's output**, including
    /// Adobe Acrobat's default view. Overprint is still honoured in full —
    /// it simply has no spot colorant left to act on, which is exactly why
    /// a white object knocks the ink out under this model and preserves it
    /// under the other.
    ///
    /// Conformant under **both** editions, unlike the default.
    AlternateSpaceSubstitution,
}

impl SpotColorantDeviceModel {
    /// Parse a settings-file / command-line token, or `None` if unknown.
    ///
    /// One vocabulary, two readers — the settings parser and the CLI flag
    /// both come here, so a token the file accepts and a token the flag
    /// accepts cannot diverge. Same argument
    /// [`OverprintZeroTintScope::parse`] makes.
    ///
    /// ```
    /// use pdfcer_core::settings::SpotColorantDeviceModel as Model;
    /// assert_eq!(Model::parse("simulate_separations"), Some(Model::SimulateSeparations));
    /// assert_eq!(
    ///     Model::parse("alternate_space_substitution"),
    ///     Some(Model::AlternateSpaceSubstitution)
    /// );
    /// assert_eq!(Model::parse("nonsense"), None);
    /// ```
    #[must_use]
    pub fn parse(token: &str) -> Option<Self> {
        match token {
            "simulate_separations" => Some(Self::SimulateSeparations),
            "alternate_space_substitution" => Some(Self::AlternateSpaceSubstitution),
            _ => None,
        }
    }

    /// The settings-file token for this value — the exact inverse of
    /// [`Self::parse`].
    ///
    /// ```
    /// use pdfcer_core::settings::SpotColorantDeviceModel as Model;
    /// for m in [Model::SimulateSeparations, Model::AlternateSpaceSubstitution] {
    ///     assert_eq!(Model::parse(m.as_str()), Some(m), "round trip");
    /// }
    /// ```
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SimulateSeparations => "simulate_separations",
            Self::AlternateSpaceSubstitution => "alternate_space_substitution",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum PageBlendSpaceSource {
    /// ISO 32000-1 §11.4.7 / §11.6.3 to the letter: the device's native
    /// space, which for pdfcer's `RGBA8` pixmap is additive.
    ///
    /// **Strictly conforming under PDF 1.7, and it renders overprint
    /// degenerately** (see the type docs). Choose this to reproduce
    /// pdfcer's pre-`Pass 122.5` output, or when the question is *"what does
    /// ISO 32000-1 literally require?"*
    DeviceNative,
    /// Consult the output intent **only when its destination profile is
    /// subtractive** (a four-or-more-colorant device class); otherwise fall
    /// back to the device's native space.
    ///
    /// **The shipped default.** Chosen on the operator's standing criterion
    /// for these — *"default it to your best guess as to what would be
    /// normally expected"* — and what is normally expected of a PDF/X print
    /// file is that it renders the way a print-oriented viewer renders it,
    /// with overprint working.
    ///
    /// The conditional is what keeps it safe: an RGB or greyscale output
    /// intent cannot drag a page into a subtractive space, so the only
    /// files this moves are ones that already declare themselves to be
    /// destined for ink.
    #[default]
    OutputIntentIfSubtractive,
    /// Consult the output intent whenever there is one, whatever its
    /// colour class.
    ///
    /// The most literal reading of Annex P's unranked *"or from the output
    /// intent"*. Kept because Annex P genuinely says this and a reader
    /// implementing it directly would land here — but not the default: an
    /// RGB output intent switching a page's blending space is a larger
    /// behavioural change than the evidence supports.
    OutputIntentAlways,
}

/// How a type 6 or type 7 mesh-shading **patch record** is padded - spec
/// ambiguity `MSH-A1` (ISO 32000-1 8.7.4.5.5/.7/.8).
///
/// # The silence being filled
///
/// 8.7.4.5.5 states the padding rule for the triangle types and scopes it
/// to a **vertex**, verbatim:
///
/// > "Each set of **vertex** data shall occupy a whole number of bytes. If
/// > the total number of bits required is not divisible by 8, the last data
/// > byte for each **vertex** is padded at the end with extra bits, which
/// > shall be ignored."
///
/// A Coons or tensor-product patch has no vertices. 8.7.4.5.7 and
/// 8.7.4.5.8 defer to that clause - *"See 8.7.4.5.5 ... for further details
/// on the format of the data"* - which **imports the rule but leaves its
/// unit undefined** for a structure the rule's own wording does not
/// describe.
///
/// Two readings survive the text, and **ISO 32000-2 does not resolve
/// either**: the spec RAG's edition delta `D3` records the sentence as
/// word-for-word identical in 2.0, including its scoping to "each vertex".
/// So this is **permanent**, not merely unresearched, which is what makes
/// it a setting rather than a defect awaiting a fix.
///
/// # When it is observable at all
///
/// Only when `BitsPerFlag + k*BitsPerCoordinate + m*BitsPerComponent` is
/// **not** a multiple of 8. The combinations measured in real files so far
/// are `8`/`32`/`8` (the print-conformance suite's two type 7 meshes) and
/// the RAG's noted common case `8`/`16`/`8`; both are byte aligned for
/// every record shape, so the two readings render identically. A file with
/// `BitsPerFlag` 2 or 4, or 12-bit coordinates, is where they diverge - and
/// there the divergence is total, because a mis-alignment desynchronises
/// every record after the first.
///
/// # Default: [`Self::PerRecord`] - the reading under which the deferral
/// has content
///
/// If 8.7.4.5.7's pointer imports nothing, it says nothing, and a normative
/// cross-reference that says nothing is the weaker of the two readings.
/// "The record" is the only structure the pointer can be importing once
/// "the vertex" does not exist.
///
/// Whichever is chosen, what pdfcer got out of the stream is disclosed
/// off-canvas (project rule 4): `pdfcer render-page` reports
/// `mesh_truncated` when a stream ended part-way through a record, which is
/// the symptom a wrong reading produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum MeshPatchPadding {
    /// Pad each **patch record** - its flag, all its coordinates and all
    /// its colours - up to a whole number of bytes, by analogy with the
    /// vertex rule. **The shipped default.**
    #[default]
    PerRecord,
    /// Do not pad patch records at all: read types 6 and 7 as a continuous
    /// bit string, on the reading that 8.7.4.5.5's rule is scoped to a
    /// structure patches do not have and therefore does not reach them.
    None,
}

/// Which filter resamples a `/SMask` or explicit `/Mask` whose pixel grid
/// differs from its base image's (spec ambiguity `SM-A1`).
///
/// # The silence being filled
///
/// ISO 32000-1 fixes the **geometry** and says nothing about the
/// **filter**. Table 145's `Width` row, verbatim: *"Both images shall be
/// mapped to the unit square in user space (as are all images),
/// **regardless of whether the samples coincide individually**."* §8.9.6.3
/// says the same for an explicit mask (*"need not have the same
/// resolution … their boundaries on the page will coincide"*).
///
/// The spec RAG records the sourced negatives that establish the silence
/// is real rather than merely unfound (`iso32000__s__11.6.5.md` § SM-A1):
/// over the whole 756-page source, `resample*` **0 hits**,
/// `nearest neigh*` **0 hits**, `bilinear` **3 hits, none image-related**.
/// §8.9.5.3's NOTE then grants a conforming reader *"any specific
/// implementation of interpolation that it wishes"*.
///
/// # Default: [`Self::Nearest`] — **EVIDENCE TIER (d)**
///
/// Tier (d) is the register's vocabulary for **reasoned inference only —
/// this is a guess and is written as one**. No tier-(a)/(b)/(c) evidence
/// exists: `Acrobat_Features` does not cover mask resampling, no census
/// has been run, and no other implementation's documented behaviour was
/// located. The reasoning (good, but still reasoning) is that
/// nearest-neighbour is the only filter that cannot invent an alpha value
/// appearing nowhere in the mask — decisive for a 1-bit stencil supplied
/// as an `/SMask`, where a blend across a 0/1 edge fabricates
/// half-transparent texels the document never asked for.
///
/// Do not read this default as evidence of what other readers do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum MaskResample {
    /// Take the single mask sample containing the base texel's centre.
    ///
    /// **The shipped default.** Never invents an alpha; preserves a
    /// stencil's hard edges exactly. Aliases (staircases) when a small
    /// mask is stretched over a large base image.
    #[default]
    Nearest,
    /// Average every mask sample the base texel's footprint covers.
    ///
    /// The right answer when the mask is *higher* resolution than the base
    /// image, where nearest-neighbour throws away most of the mask: a
    /// 4× mask read one-sample-per-texel discards fifteen sixteenths of
    /// what the producer supplied. Degenerates to [`Self::Nearest`] when
    /// the footprint covers one sample.
    BoxAverage,
    /// Interpolate linearly between the four mask samples nearest the base
    /// texel's centre.
    ///
    /// Smooth on magnification, which is what makes it the wrong default:
    /// across a stencil's 0↔255 boundary it manufactures intermediate
    /// alphas. Offered for a continuous-tone `/SMask` (a photographic
    /// vignette) supplied at lower resolution than its base image, which
    /// is the case it is actually good at.
    Bilinear,
}

/// How an image XObject is sampled when it is drawn **smaller** than its
/// own pixel grid (spec ambiguity `IM-A1`).
///
/// # The silence being filled
///
/// §8.9.5.3 (*Image Interpolation*) defines interpolation **only for
/// magnification** — *"When the resolution of a source image is
/// significantly **lower** than that of the output device …"* — and its
/// NOTE grants a reader leave to *"not implement this feature"* or to
/// *"use any specific implementation of interpolation that it wishes"*.
///
/// It says nothing at all about minification. Term-frequency evidence over
/// the source (`iso32000__ref__ambiguity_settings_register.md` §5.5):
/// `minif` **0 hits**, `mipmap` **0**, `decimat` **0**, `down-sampl` **0**,
/// `downsampl` **2 hits, both unrelated** (multimedia rate conversion in
/// clause 13; the thumbnail note in §8.9.5.4). So `/Interpolate false`
/// does **not** mandate point-sampling on the way *down* — it switches off
/// the *up*-scaling smoothing the clause actually defines, and a reader
/// minifying an image is unconstrained.
///
/// # Default: [`Self::Smooth`] — **EVIDENCE TIER (c)**, flipped 2026-08-25
///
/// ★ **This default changed, and it changed because the condition written
/// into this very comment was met rather than because anybody argued for
/// it.** The prior text read:
///
/// > ~~"Default: `PointSample` — EVIDENCE TIER (d). Tier (d): reasoned
/// > inference only, i.e. **a guess** … A viewer-behaviour check filed to
/// > `C:\personal_rag\pdf\` would raise this to tier (c) and, if it
/// > confirms, flip the default. Until then the status quo stands and is
/// > labelled a guess."~~
///
/// The check was run. The operator compared pdfcer against **Acrobat
/// Reader**, on his own CAD drawings, on his own screen, and reported
/// unprompted that image quality on ordinary pages was *"a little worse
/// than it was, whereas before it was on par with Acrobat Reader"*.
///
/// ★★ **The load-bearing detail is that he described the MECHANISM from
/// the symptom, without being told it existed** — *"an image quality
/// setting to discard smaller details than the screen sees"* is
/// [`Self::PointSample`], exactly. A report that names the mechanism
/// unprompted is a stronger observation than one that agrees with a
/// hypothesis it was handed, because it cannot have been led.
///
/// So this is now tier (c) — *what another major implementation does,
/// observed* — and tier (c) is the bar this comment set for itself.
///
/// # What did NOT happen, recorded so nobody hunts for a commit
///
/// **It was not a regression.** `PointSample` had been the shipped default
/// throughout, and `RenderOptions::default()` carried the same
/// `MinifyFilter::default()`, so routing the setting through a GUI control
/// changed no pixels. Both halves were verified before acting. There was
/// no revert to find; there was a default to decide.
///
/// # The residual, stated rather than smoothed over
///
/// [`Self::PointSample`] remains the **spec-literal** reading — §8.9.5.3
/// legislates only magnification, so nothing in the clause is being
/// contradicted in either direction. What is being chosen is what a viewer
/// should do where the standard is silent, and the answer is now *"what
/// the reference viewer visibly does"* instead of *"the narrowest reading
/// of a switch that governs the other direction"*.
///
/// A test that asserts exact pixels on a minified image will change under
/// this default. That is the point of it, not a side effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum MinifyFilter {
    /// Take one texel per output pixel, in both directions — treat
    /// `/Interpolate` as the only switch there is.
    ///
    /// **Spec-literal**, and it is what makes a 2×2 test image's pixels
    /// exactly assertable — which is why several of this project's own
    /// image tests select it explicitly rather than relying on the
    /// default. Its cost is aliasing (shimmer, dropped hairlines) on a
    /// heavily downscaled image, and that cost is what the operator saw.
    ///
    /// No longer the default; see the type docs for the observation that
    /// moved it.
    PointSample,
    /// Smooth when the image is drawn smaller than its pixel grid, while
    /// still honouring `/Interpolate` on the way up.
    ///
    /// **The shipped default** as of 2026-08-25. Removes the aliasing at
    /// the price of a departure from the clause's stated switch — which is
    /// legitimate precisely because the clause never legislated this
    /// direction (§8.9.5.3 defines interpolation for MAGNIFICATION only).
    #[default]
    Smooth,
}

/// How to read a four-component `DCTDecode` image that declares no
/// `/Decode` array (spec ambiguity `DCT-A1`).
///
/// # The question
///
/// A CMYK JPEG with **effective `ColorTransform` 0** and **no `/Decode`**:
/// are the stored samples direct CMYK, or Adobe-complemented CMYK? Nothing
/// in the codestream or the image dictionary disambiguates it — the
/// undocumented 1990s Photoshop convention stores complemented values, and
/// there is no marker bit that says so.
///
/// # Default: [`Self::NeverInvert`] — **EVIDENCE TIER (c)**
///
/// Tier (c) means *what other major implementations do, as documented* —
/// and this is the **strongest-sourced default in the whole ambiguity
/// register**, the one place it is not a guess:
///
/// - the word `"invert"` occurs **zero times** in Adobe TN #5116, the
///   document ISO 32000-1 §7.4.8 footnote *a* makes normative by
///   reference (verified 2026-07-31);
/// - **APP14 carries no polarity flag** — there is no bit to test, so
///   "invert when the marker is present" keys off mere presence;
/// - `filter__dct.md` records that all four reference engines accept the
///   ambiguity rather than inverting on APP14 presence.
///
/// This is also pdfcer's standing rule **R29** (decision 006), and the
/// residual risk is already disclosed rather than repaired by
/// [`crate::image_codec::CodecNotes::cmyk_polarity_unverifiable`] (R30).
/// The setting adds the operator's escape hatch; it does not weaken R29,
/// which remains what pdfcer does unless the operator says otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum CmykJpegPolarity {
    /// Take the samples as stored. `/Decode` is the sole polarity control
    /// (`/Decode [1 0 1 0 1 0 1 0]` *is* the sanctioned way for a producer
    /// to declare inverted storage).
    ///
    /// **The shipped default**, and the standing rule.
    #[default]
    NeverInvert,
    /// Complement all four components (`255 − x`) when the codestream
    /// carries an Adobe APP14 marker, the effective transform is 0, and
    /// the image dictionary declares no `/Decode`.
    ///
    /// For a library of old Photoshop-authored CMYK JPEGs that genuinely
    /// do store complemented ink and say so nowhere. Getting this wrong in
    /// either direction renders a photographic negative — which is at
    /// least an obvious failure, not a subtle one.
    InvertOnApp14,
}

/// What character extraction emits for a code no rung of the §9.10.2
/// ladder could map (spec ambiguity `TX-A1`).
///
/// # The silence being filled
///
/// §9.10.2's failure clause is *grammatically broken* — it says a
/// conforming reader *"may choose a character code of their choosing"*
/// where a **Unicode value** is what is being produced — and **no
/// sentinel is specified anywhere in the standard**: not U+FFFD, not
/// omission, not a placeholder.
///
/// # Default: [`Self::ReplacementChar`] — **EVIDENCE TIER (d)**
///
/// Tier (d): reasoned inference only — **a guess**. The reasoning is that
/// U+FFFD is the only option that is simultaneously length-preserving
/// *and* visibly wrong, which is what rule 4 wants; omission silently
/// shortens the text and makes the failure invisible. No census, no
/// Acrobat citation, no documented third-party behaviour backs it.
///
/// # This is an EXTRACT-radius setting, which makes it a correctness knob
///
/// Downstream of extraction sit search, clipboard copy, **and
/// redaction-by-text**. Changing the sentinel changes character offsets,
/// therefore changes which runs a redaction pattern matches (**R35**). A
/// redaction built under one value is not equivalent under another.
/// Whatever is chosen, the rung-4 counter keeps counting — that counter is
/// documented as *"the headline honesty metric"* and the setting must not
/// be able to switch it off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum UnmappableCode {
    /// U+FFFD REPLACEMENT CHARACTER, one per unmappable code.
    ///
    /// **The shipped default.** Length-preserving and visibly wrong.
    #[default]
    ReplacementChar,
    /// `?`, one per unmappable code.
    ///
    /// Also length-preserving, but it survives being pasted into tools
    /// that mangle U+FFFD, and it reads as a question rather than as a
    /// font problem. It is *less* honest than U+FFFD in one specific way:
    /// a genuine `?` in the document is indistinguishable from a failure.
    QuestionMark,
    /// Nothing at all — the code contributes no characters.
    ///
    /// The failure is still counted (`ladder_failures`), so it is never
    /// hidden from the operator; only the text is shorter, and the
    /// shortening is invisible **in the text itself**. Choose this when
    /// the extracted text is being fed to something that chokes on
    /// sentinels.
    ///
    /// **Two consequences worth knowing before choosing it**, both
    /// measured rather than assumed:
    ///
    /// 1. **Character offsets move**, so a search hit and a
    ///    redaction-by-text match land in different places than they do
    ///    under the other two values (R35). That is true of any change
    ///    here, but `omit` is the one that changes them the most.
    /// 2. **A run whose codes are ALL unmappable disappears entirely** —
    ///    glyph records included. The layout pass drops a run with no
    ///    characters (it has nothing a caller can index into), so under
    ///    `omit` a page of `Identity-H` text with no `/ToUnicode` yields
    ///    zero runs rather than runs of sentinels. A caller that needs
    ///    per-glyph positions for unmappable codes must not choose this.
    ///    Pinned by
    ///    `the_unmappable_sentinel_changes_the_characters_but_never_the_count`.
    ///
    /// **Scope: extraction output only.** Three internal paths pin the
    /// sentinel to [`Self::ReplacementChar`] regardless, because in each
    /// of them a zero-length character would break something structural
    /// rather than merely look different — the text-editing slot table
    /// (a zero-length span is a glyph the operator can see and cannot
    /// address), the redaction audit record (which must not report a
    /// removal as nothing), and the vector-object text preview (which must
    /// not make an undecodable run look empty). Each site says so at the
    /// call.
    Omit,
}

/// Whether `/ActualText` replaces the glyph-derived characters
/// (spec ambiguity `AT-A1`).
///
/// # The disagreement being resolved
///
/// Three statements in ISO 32000-1 do not agree, and none dislodges the
/// others:
///
/// - **§14.9.4**: `/ActualText` *"shall be used as a replacement"* — the
///   only **`shall`** in the set.
/// - **§14.8.2.4.2 NOTE 2**: readers *"may choose to use"* it, and *"some
///   conforming readers"* do — a `may`, inside an **informative NOTE**.
/// - **§9.10.1**: it *"may be used"*.
///
/// The only sentence that addresses precedence is the `may`, and it sits
/// in a NOTE, so neither reading can be eliminated from the standard.
///
/// # Default: [`Self::Always`] — **EVIDENCE TIER (d)**
///
/// Tier (d) — **a guess**, though the best-supported guess available:
/// §14.9.4's is the only `shall`, and its competitors are a NOTE and a
/// `may`. Per the standing normative-vs-informative rule, the NOTE is
/// **not** cited alone as authority anywhere in the code.
///
/// # A bound that is NOT a setting
///
/// **No length correspondence exists** between `/ActualText` and the
/// content it replaces — the standard's own example maps two shown
/// characters to one. Character-level mapping back to glyph positions is
/// therefore *impossible* across an `/ActualText` run, which bounds
/// search-highlight, selection and redaction-by-text to **sequence**
/// granularity whichever value is chosen. That is a fact to disclose, not
/// a direction to pick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ActualTextPrecedence {
    /// `/ActualText` replaces the glyphs it covers, wherever it appears.
    ///
    /// **The shipped default.** §14.9.4's `shall`, applied literally.
    #[default]
    Always,
    /// `/ActualText` replaces the glyphs only when the marked-content
    /// sequence carrying it is part of the structure tree.
    ///
    /// "Part of the structure tree" is tested as **an `/MCID` in scope** —
    /// on the sequence itself or on an enclosing one. That is the only
    /// test available inside a content stream: `/MCID` is precisely what
    /// §14.7.4.2 uses to join a marked-content sequence to a structure
    /// element, so a sequence without one in scope is not tagged content
    /// in any sense the page itself can express. Elsewhere the glyphs win.
    ///
    /// Choose this when a producer sprinkles `/ActualText` outside its
    /// tagged content and the replacements are worse than the glyphs.
    TaggedOnly,
    /// The glyphs always win; `/ActualText` is counted and reported but
    /// never substituted.
    ///
    /// The forensic setting: what is extracted is what the page draws.
    /// Note that this **loses** genuinely unrecoverable text — a ligature
    /// whose only Unicode identity was in its `/ActualText` extracts as
    /// whatever the ladder makes of the glyph, which may be U+FFFD.
    Glyphs,
}

/// What to paint for an annotation whose `/AP` `/N` is a subdictionary of
/// two or more entries and which carries **no `/AS`**
/// (spec ambiguity `AS-A1`).
///
/// # The gap being filled
///
/// Table 164 makes `/AS` *required* in exactly that configuration, so such
/// a file is **malformed**. §12.5.5 NOTE 3 covers only the neighbouring
/// case — `/AS` present but naming an absent state — and states no
/// recovery for `/AS` being absent altogether.
///
/// A single-entry subdictionary is **not** covered by this setting and
/// never was: with one entry there are no alternatives to choose between,
/// so painting it is not a guess. The forbidden case is specifically the
/// multi-entry one.
///
/// # Default: [`Self::PaintNothing`] — **EVIDENCE TIER (d)**
///
/// Tier (d) — **a guess**, and deliberately the conservative one. The spec
/// RAG's row is explicit that the other two options are *empirical*
/// guesses belonging to `C:\personal_rag\pdf\`: *"do NOT silently pick
/// first/`Off`/`On`."* Offering them as opt-ins is legitimate; making one
/// the installed default would be exactly the "sneaky" failure rule 4
/// forbids, because the operator would see a plausible appearance with no
/// indication that pdfcer chose it.
///
/// Whatever is chosen, the case stays **counted** — pdfcer never repairs
/// the file by writing an `/AS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum MissingAppearanceState {
    /// Paint nothing, and count the annotation as state-unresolved.
    ///
    /// **The shipped default.**
    #[default]
    PaintNothing,
    /// Paint the subdictionary's first entry in key order.
    ///
    /// "First" is the dictionary's own iteration order, which pdfcer
    /// preserves from the file, so this is *the producer's* first entry
    /// and not an alphabetical invention.
    FirstEntry,
    /// Paint the `/Off` entry if there is one, otherwise nothing.
    ///
    /// The checkbox-shaped guess: for a widget the unchecked state is the
    /// one that misleads least if it is wrong.
    OffElseNothing,
}

/// Which corner order pdfcer writes into `/QuadPoints` (§12.5.6.10) —
/// ambiguity **`QP-A1`**.
///
/// # The ambiguity
///
/// §12.5.6.10 states a corner order, and **essentially no producer follows
/// it.** Acrobat, PDFBox and pdf.js all emit `Z` / reading order — upper-left,
/// upper-right, lower-left, lower-right — while the clause describes a
/// counterclockwise walk, which swaps the last two corners.
///
/// The ambiguity register calls this the **worst case in its table**, and
/// the reason is worth keeping: it is a deliberate divergence from a
/// `shall`-adjacent normative statement, and it is **invisible at runtime**.
/// pdfcer bakes a full `/AP` (R44), so its own rendering never consults
/// `/QuadPoints` — the order matters only to a *third-party* consumer that
/// re-derives geometry from it, and a wrong order there produces a bow-tie
/// rather than a rectangle.
///
/// # Why this is a setting rather than a fixed choice
///
/// The two readings serve genuinely different operators. Someone marking up
/// a document for colleagues wants it to look right in **Acrobat**; someone
/// producing a file for conformance checking wants it to match **the clause**.
/// Neither is wrong, and the standard does not adjudicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum QuadPointOrder {
    /// `UL, UR, LL, LR` — what Acrobat, PDFBox and pdf.js emit and expect.
    ///
    /// **The shipped default**, chosen for interoperability: a markup
    /// annotation is read by whatever the recipient already has, and that is
    /// overwhelmingly one of these three. A file that is spec-literal and
    /// draws a bow-tie in the reader the recipient actually opened has
    /// helped nobody.
    #[default]
    ReadingOrder,
    /// `UL, UR, LR, LL` — the counterclockwise walk §12.5.6.10 describes.
    ///
    /// For output destined for a conformance checker or a consumer that
    /// implements the clause literally. Expect Acrobat to render markup
    /// geometry re-derived from these quads incorrectly.
    Counterclockwise,
}

/// Which of §7.5.4's three permitted two-byte terminators ends a classic
/// cross-reference **entry** (spec ambiguity `EOL-A1`).
///
/// # The choice being made
///
/// §7.5.4 fixes the entry at exactly 20 bytes and permits three, and only
/// three, forms for bytes 18–19. `LF CR`, a bare `LF`, a bare `CR`,
/// `SP SP` and `SP CR LF` are **not** legal and are deliberately not
/// offered here — a settings file is not a licence to emit a
/// non-conforming file.
///
/// # Default: [`Self::MatchSource`] — the register's own recommendation
///
/// **Changed on the operator's ruling of 2026-08-08** ("change the shipped
/// default so that we match the file's existing 2-byte EOL"), replacing a
/// fixed `SP LF`.
///
/// `iso32000__ref__ambiguity_settings_register.md` §5.11 recommended
/// exactly this and pdfcer shipped the fixed form anyway, because
/// implementing "match the source" needed an observation of the base
/// file's bytes that no channel carried. The register said plainly that
/// the shipped default was *"arguably wrong on pdfcer's own invariant"*,
/// and it was right: **rule 3 says objects pdfcer did not logically touch
/// are re-emitted byte-identical, and a full rewrite of a `CR LF` file
/// under a fixed `SP LF` changes two bytes in every entry of the table.**
/// On a 5,000-object file that is a 10,000-byte diff in a document nobody
/// edited — the exact diff minimal-diff editing exists to prevent.
///
/// The channel now exists: [`crate::xref::observed_entry_eol`] reads the
/// form back out of the base file, and the writer resolves
/// [`Self::MatchSource`] against it. This is the same idea
/// `Document::section_shape` already served at a coarser grain — *the base
/// file's own form* (R33) — one level finer.
///
/// **Evidence tier is no longer (d)-shaped at all**, which is the quiet
/// win here. The old default rested on the RAG's uncited claim that
/// `SP LF` is *"the common choice"* — flagged in the register's §11.3 as
/// carrying no source and pending a downgrade. The new default rests on
/// nothing external: it derives the answer from the file in front of it,
/// so there is no guess left to grade. The uncited claim now governs only
/// the fallback, where there is genuinely nothing to match.
///
/// **BYTES blast radius, zero render effect.** Every value is conforming,
/// so no operator disclosure is needed when one is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum XrefEntryEol {
    /// Write whichever of the three forms the file being saved already
    /// used, falling back to `SP LF` when there is nothing to match.
    ///
    /// **The shipped default.** "Nothing to match" means a
    /// cross-reference *stream* file (§7.5.8 is binary and has no entry
    /// EOL), a file whose table is non-conforming at that position, or a
    /// document pdfcer assembled from nothing — see
    /// [`crate::xref::observed_entry_eol`].
    #[default]
    MatchSource,
    /// Always `SP LF` (`20 0A`), whatever the source used.
    SpaceLf,
    /// Always `SP CR` (`20 0D`).
    SpaceCr,
    /// Always `CR LF` (`0D 0A`).
    CrLf,
}

impl XrefEntryEol {
    /// The concrete two bytes to emit, resolving [`Self::MatchSource`]
    /// against the file being saved.
    ///
    /// `base` is the bytes of the document this save is derived from —
    /// empty for a document assembled from nothing. Kept as a method on
    /// the setting rather than a branch in the writer so that every
    /// caller resolves it the same way; a second resolution site is how
    /// an incremental save and a full rewrite would come to disagree
    /// about the same file.
    #[must_use]
    pub fn resolve(self, base: &[u8]) -> Self {
        match self {
            Self::MatchSource => crate::xref::observed_entry_eol(base).unwrap_or(Self::SpaceLf),
            other => other,
        }
    }

    /// The two bytes themselves. [`Self::MatchSource`] resolves to the
    /// fallback here, so callers that have a base file must call
    /// [`Self::resolve`] first.
    #[must_use]
    pub const fn bytes(self) -> [u8; 2] {
        match self {
            Self::SpaceLf | Self::MatchSource => *b" \n",
            Self::SpaceCr => *b" \r",
            Self::CrLf => *b"\r\n",
        }
    }
}

/// Whether the writer puts an end-of-line byte after the final `%%EOF`
/// (spec ambiguity `EOL-A2`).
///
/// # The disagreement being resolved
///
/// §7.5.1 requires every line to be EOL-terminated; §7.5.5 says the last
/// line *"contains only"* `%%EOF`. **Both readings are self-consistent and
/// the standard does not choose between them.**
///
/// # Default: [`Self::Lf`] — **EVIDENCE TIER (d)**
///
/// Tier (d) — **a guess**, and the safe side of one: §7.2.3 requires the
/// incremental-append path to have an EOL before a following `12 0 obj`
/// anyway, and a trailing EOL never breaks a reader's backward `%%EOF`
/// scan. Low value as a knob; it exists because the choice is currently
/// hard-coded, is labelled in the source as a recorded spec ambiguity, and
/// an engineer who finds that label will ask where the switch is.
///
/// **BYTES blast radius — one byte.** No disclosure needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum TrailingEol {
    /// Terminate the `%%EOF` line with `LF`. **The shipped default.**
    #[default]
    Lf,
    /// End the file at the final `F` of `%%EOF`.
    None,
}

/// The operator's persisted choices.
///
/// Deliberately a flat struct of plain values. Grouping into
/// sub-structures would make the file format hierarchical, and the format
/// is flat on purpose (see the module docs).
///
/// # Adding a setting
///
/// Four edits, all in this file, and the compiler finds three of them:
/// a field here, a line in [`Settings::apply`], a line in
/// [`Settings::write_to_string`], and a row in the round-trip test. The
/// default belongs on the *type*, not here.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Settings {
    /// What happens when a page operation splits a preseparated page set
    /// (§14.11.4).
    ///
    /// Note this is a **product policy**, not a spec ambiguity — §14.11.4
    /// is perfectly clear about the invariant and simply does not say what
    /// an editor should do when an edit breaks it. It is a setting because
    /// all three answers are defensible for different workflows, not
    /// because the standard is unclear.
    pub separations: SeparationPolicy,
    /// What pdfcer does when a bold/italic request may need a fallback
    /// (`Pass 179.0`). Defaults to [`StylePolicy::Auto`] — decide and apply,
    /// silently.
    pub style_policy: StylePolicy,
    /// How `DeviceCMYK` is converted for display.
    pub cmyk_intent: CmykIntent,
    /// The largest **subtractive compositing buffer** the renderer may
    /// allocate, in bytes. `None` = the renderer's built-in default.
    ///
    /// # Why this is the operator's number and not a constant
    ///
    /// A page whose group declares a subtractive blending space
    /// (ISO 32000-1 §11.4.7) is composited in a four-colorant buffer, and
    /// that buffer costs 20 bytes per pixel — so it grows with the square of
    /// the zoom. Past a ceiling the renderer composites on screen instead
    /// and discloses that it did. The consequence is operator-visible and is
    /// what prompted this field: **the same page rendered different colours
    /// at different zoom levels**, crossing the built-in ceiling at about
    /// 518 % on A4, with nothing on screen able to say where the boundary
    /// was because nothing outside the renderer could read it.
    ///
    /// The right value is a function of the operator's screen and their
    /// tolerance for memory — a 4K viewport with overscan wants ~633 MB
    /// where a 1600×900 one wants ~110 MB — and **neither is knowable from
    /// inside the renderer.** So it is a setting, uncapped, on the same
    /// ruling the operator gave for the zoom ceiling: *"it is up to the user
    /// to determine how much of a performance hit they want to take."*
    ///
    /// # It is uncapped, and that is safe for a specific reason
    ///
    /// `ARCHITECTURE.md` §10 forbids an **untrusted-input-sized**
    /// allocation without a ceiling. A page's dimensions are untrusted
    /// input; a number the operator typed is not. A value larger than the
    /// machine can supply is not a crash either — the renderer attempts the
    /// allocation fallibly and falls back to the disclosed sRGB path, the
    /// same as if the ceiling had refused.
    ///
    /// Raising it costs time as well as memory: compositing in ink measured
    /// roughly 50 % slower than compositing on screen at the same pixel
    /// count.
    pub max_cmyk_buffer_bytes: Option<usize>,
    /// Which visual theme the GUI uses, as an opaque token.
    ///
    /// # A `String`, deliberately, and core does not validate it
    ///
    /// The set of themes is a **shell** concern — `pdfce-gui` owns the
    /// palettes, and `pdfcer-core` must never gain a GUI dependency
    /// (`ARCHITECTURE.md` §3, the invariant that keeps a future WASM
    /// fork a shell swap rather than a rewrite). An enum here would put
    /// the shell's vocabulary in the core crate for no benefit, and
    /// would have to be extended in core every time the shell added a
    /// look.
    ///
    /// So core stores and round-trips the token and takes no view on
    /// what it means. The shell resolves it, and is responsible for
    /// saying so when it cannot — an unknown token is a note the
    /// operator sees, not a silent reset, because silently discarding a
    /// preference is indistinguishable from losing it.
    ///
    /// A consequence worth having: a settings file written by a NEWER
    /// pdfcer keeps its theme when an older one opens and re-saves it.
    pub theme: String,
    /// The gap, as a multiple of the current font size, at which
    /// text extraction inserts a word break.
    ///
    /// Already existed as `ExtractOptions::word_gap_ratio` with a
    /// documented default and a builder — and with **zero** CLI and GUI
    /// callers, which is what made it the register's cheapest win: the
    /// setting was built, just unreachable.
    pub word_gap_ratio: f32,
    /// How many degrees apart two lines may be and still be dimensioned as
    /// PARALLEL rather than as an angle (ce dimensions, two-line pick).
    ///
    /// # Why this is a setting and not a constant
    ///
    /// Nothing defines it. A search of the SolidWorks dimension/tolerance
    /// corpus at `D:\Dev\Rag-Specialized\SolidWorks_Dimensions\` for an
    /// epsilon, a threshold or a near-parallel snap rule found none — the
    /// catalog records the whole question as unverified. Standing rule R169
    /// says a choice no standard makes is a setting rather than a number
    /// buried in the geometry, and this is exactly that case.
    ///
    /// The default of half a degree is a judgement and is documented as one:
    /// CAD-exported geometry is usually exact, so a pair a hair off parallel
    /// is far more likely to be an exporter's rounding artefact than a
    /// deliberate shallow taper. An operator who genuinely dimensions
    /// shallow tapers should lower it.
    ///
    /// This governs only the AUTOMATIC classification. The operator can
    /// always force the parallel reading for one specific ce dimension —
    /// see the two-line authoring surface — so a wrong global default costs
    /// a checkbox, never the ability to get the dimension they want.
    pub parallel_epsilon_degrees: f64,
    /// Which filter resamples a size-mismatched `/SMask` or `/Mask`
    /// (`SM-A1`, §8.9.6.3 / Table 145). RENDER radius.
    /// Where a page's blending colour space comes from when its group
    /// declares none — spec ambiguity `PGB-A1`. See
    /// [`PageBlendSpaceSource`], whose docs carry the clause citations
    /// and the reason this is a setting rather than a fix.
    pub page_blend_space_source: PageBlendSpaceSource,
    /// Which colour spaces get `OPM 1`'s zero-tint rule (`Pass 143.0`).
    /// See [`OverprintZeroTintScope`] — a **divergence from ISO 32000-1**
    /// toward Acrobat, edition-gated (32000-2 opens the question that 1.7
    /// answers). This line said *"the §8.6.7 ambiguity"* until `Pass 174.6`,
    /// **960 lines below the block `Pass 174.5` corrected and in the same
    /// file** — `R234`'s own worked example.
    pub overprint_zero_tint_scope: OverprintZeroTintScope,
    /// Which output-device model a spot colorant is rendered against —
    /// spec fork `OP-A7`. See [`SpotColorantDeviceModel`]; both values are
    /// conformant and they render a spot backdrop under overprint
    /// differently.
    pub spot_colorant_device_model: SpotColorantDeviceModel,
    /// How a type 6/7 mesh-shading patch record is byte-padded - spec
    /// ambiguity `MSH-A1`, 8.7.4.5.5/.7/.8. See [`MeshPatchPadding`], whose
    /// docs carry the clause text and the reason it is permanent rather
    /// than pending. RENDER radius.
    pub mesh_patch_padding: MeshPatchPadding,
    pub mask_resample: MaskResample,
    /// How an image drawn smaller than its own pixel grid is sampled
    /// (`IM-A1`, §8.9.5.3). RENDER radius.
    pub image_minify: MinifyFilter,
    /// How a CMYK JPEG that declares no `/Decode` is read (`DCT-A1`,
    /// §7.4.8 + Table 13). RENDER radius, and BYTES wherever pdfcer
    /// re-encodes — a re-encode under the wrong polarity bakes the
    /// inversion in permanently.
    pub cmyk_jpeg_polarity: CmykJpegPolarity,
    /// What extraction emits for a code the §9.10.2 ladder cannot map
    /// (`TX-A1`). **EXTRACT radius** — it moves character offsets, so it
    /// moves redaction-by-text coverage (R35).
    pub unmappable_code: UnmappableCode,
    /// Whether `/ActualText` replaces the glyphs it covers (`AT-A1`,
    /// §14.9.4). **EXTRACT radius**, same R35 note as
    /// [`Self::unmappable_code`].
    pub actual_text: ActualTextPrecedence,
    /// What to paint for a multi-entry `/AP` `/N` subdictionary with no
    /// `/AS` (`AS-A1`, §12.5.5). RENDER radius only — pdfcer never writes
    /// an `/AS` to repair the file.
    pub missing_as: MissingAppearanceState,
    /// The two-byte terminator on a classic cross-reference entry
    /// (`EOL-A1`, §7.5.4). **BYTES radius.**
    /// `/QuadPoints` corner order for authored text markup — ambiguity
    /// `QP-A1`. Key `quad_point_order`.
    pub quad_point_order: QuadPointOrder,
    pub xref_entry_eol: XrefEntryEol,
    /// Whether a byte follows the final `%%EOF` (`EOL-A2`, §7.5.5).
    /// **BYTES radius** — one byte.
    pub trailing_eol: TrailingEol,
}

impl Default for Settings {
    /// Every default, **taken from the type that owns it** rather than
    /// restated here.
    ///
    /// `#[derive(Default)]` was the obvious choice and it was wrong: it
    /// gives `word_gap_ratio = 0.0`, because `f32`'s default is zero and
    /// the engine's default is `0.20`. That is the exact failure mode this
    /// module warns about in its own docs — one answer to "what does pdfcer
    /// do by default?", not two — and it shipped for about ten minutes
    /// until `every_setting_round_trips_through_the_file` caught it.
    ///
    /// Reading the value off [`ExtractOptions::default`] rather than
    /// copying the number means the two cannot drift at all, which is
    /// strictly better than a mirrored constant plus a test asserting the
    /// mirror still holds.
    fn default() -> Self {
        Self {
            separations: SeparationPolicy::default(),
            style_policy: StylePolicy::default(),
            cmyk_intent: CmykIntent::default(),
            // `None`, and deliberately NOT a copy of the renderer's
            // constant: `pdfcer-core` cannot see `pdfcer-render` (the
            // dependency runs the other way), and a mirrored number is the
            // exact drift this module's `word_gap_ratio` default already
            // demonstrated once. `None` means "whatever the renderer's
            // default is", which is true forever without being restated.
            max_cmyk_buffer_bytes: None,
            // The shell's default preset name, as a literal rather than
            // an import, for the layering reason on the field. The GUI's
            // `theme::Preset::default().key()` must agree, and
            // `the_core_default_theme_token_is_one_the_shell_knows` in
            // `pdfce-gui` is what checks that it does.
            theme: "quiet".to_owned(),
            word_gap_ratio: crate::text_extract::ExtractOptions::default().word_gap_ratio,
            // Taken from the geometry module's own policy default rather than
            // restated, so the settings file and the classifier cannot come to
            // disagree about the same number — the failure this module's
            // `word_gap_ratio` default already demonstrated once.
            parallel_epsilon_degrees: crate::vector::linepick::ParallelPolicy::default()
                .epsilon_degrees,
            // The ambiguity-register enums declare their own default on
            // the variant, the same way `CmykIntent` does, because the
            // *choice* is the thing they exist to model — there is no
            // other type that "owns the behaviour" for, say, a mask
            // resampling filter. The consuming option structs
            // (`ExtractOptions`, `RenderOptions`, `SaveOptions`) read
            // `Enum::default()` in turn, so there is still exactly one
            // answer to "what does pdfcer do by default?", and tests in
            // this module and in `pdfcer-render` pin that agreement.
            page_blend_space_source: PageBlendSpaceSource::default(),
            overprint_zero_tint_scope: OverprintZeroTintScope::default(),
            spot_colorant_device_model: SpotColorantDeviceModel::default(),
            mesh_patch_padding: MeshPatchPadding::default(),
            mask_resample: MaskResample::default(),
            image_minify: MinifyFilter::default(),
            cmyk_jpeg_polarity: CmykJpegPolarity::default(),
            unmappable_code: crate::text_extract::ExtractOptions::default().unmappable_code,
            actual_text: crate::text_extract::ExtractOptions::default().actual_text,
            missing_as: MissingAppearanceState::default(),
            quad_point_order: QuadPointOrder::default(),
            xref_entry_eol: crate::writer::SaveOptions::default().xref_entry_eol,
            trailing_eol: crate::writer::SaveOptions::default().trailing_eol,
        }
    }
}

/// A byte size pdfcer could not read.
///
/// Carries the offending text because the shell that reports it usually
/// has nothing else to show the operator — the settings file reports the
/// key and line itself through [`SettingNote::BadValue`], but a value typed
/// into a settings window or passed on a command line has neither.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "{value:?} is not a size: write a number of bytes, a size such as `256mib` or `1.5gb`, or `default`"
)]
#[non_exhaustive]
pub struct ByteSizeError {
    /// The text as the operator wrote it, trimmed.
    pub value: String,
}

/// Parse a byte size written the way a person writes one.
///
/// `Ok(None)` is the literal `default` — *"understood, and it means
/// unset"* — so the return type is exactly the type of the setting it
/// fills ([`Settings::max_cmyk_buffer_bytes`]) and a caller never has to
/// translate.
///
/// Public because a shell offering the ceiling in a settings window must
/// accept the **same** spellings the file does. A second parser would be a
/// second vocabulary, and then a value the operator can type into pdfcer is
/// one pdfcer's own file rejects.
///
/// # Errors
///
/// [`ByteSizeError`] for a negative, non-finite, empty, unparseable or
/// absurdly large value, or an unknown suffix.
///
/// ```
/// # use pdfcer_core::settings::parse_byte_size;
/// assert_eq!(parse_byte_size("default"), Ok(None));
/// assert_eq!(parse_byte_size("256mib"), Ok(Some(268_435_456)));
/// assert_eq!(parse_byte_size("0.25gb"), Ok(Some(268_435_456)));
/// assert_eq!(parse_byte_size("268435456"), Ok(Some(268_435_456)));
/// assert!(parse_byte_size("plenty").is_err());
/// ```
///
/// # Accepted forms, and why more than one
///
/// A bare integer is bytes. A `mb` / `gb` / `mib` / `gib` suffix (any
/// case, optional space) multiplies. Both are defensible and there is no
/// reason to make the operator guess which one this file wants, so both
/// are accepted — `268435456`, `256mb`, `256 MiB` and `0.25gb` are the
/// same value.
///
/// **`mb` and `mib` both mean 1,048,576 here.** That is not sloppiness
/// about SI: this number sizes an allocation, every figure pdfcer reports
/// about it is binary, and an operator who writes `512mb` after reading a
/// ceiling described as 256 MB would otherwise get 488 MiB and a
/// disclosure that disagreed with their own arithmetic. Being consistent
/// with the rest of pdfcer beats being right about a prefix nobody typed
/// deliberately.
///
/// A fractional value is accepted (`1.5gb`) and truncated toward zero,
/// because a size in gigabytes is the one place a decimal point is the
/// natural way to write it.
///
/// # Failure conditions
///
/// A negative number, a non-finite one, an unknown suffix, an empty
/// string, or a value too large for `usize` all return `None`. **Zero is
/// accepted**, and means the ceiling refuses every buffer — a legitimate
/// way to say *"never composite in ink"* and see what that costs, rather
/// than a degenerate value worth rejecting.
pub fn parse_byte_size(value: &str) -> Result<Option<usize>, ByteSizeError> {
    let bad = || ByteSizeError {
        value: value.trim().to_owned(),
    };
    let v = value.trim();
    if v.eq_ignore_ascii_case("default") {
        return Ok(None);
    }
    let lower = v.to_ascii_lowercase();
    let (number, multiplier) = if let Some(rest) = lower.strip_suffix("gib") {
        (rest, 1024.0 * 1024.0 * 1024.0)
    } else if let Some(rest) = lower.strip_suffix("mib") {
        (rest, 1024.0 * 1024.0)
    } else if let Some(rest) = lower.strip_suffix("kib") {
        (rest, 1024.0)
    } else if let Some(rest) = lower.strip_suffix("gb") {
        (rest, 1024.0 * 1024.0 * 1024.0)
    } else if let Some(rest) = lower.strip_suffix("mb") {
        (rest, 1024.0 * 1024.0)
    } else if let Some(rest) = lower.strip_suffix("kb") {
        (rest, 1024.0)
    } else if let Some(rest) = lower.strip_suffix('b') {
        (rest, 1.0)
    } else {
        (lower.as_str(), 1.0)
    };
    let parsed: f64 = number.trim().parse().map_err(|_| bad())?;
    if !parsed.is_finite() || parsed < 0.0 {
        return Err(bad());
    }
    let bytes = parsed * multiplier;
    // `usize::MAX as f64` rounds UP, so comparing against it directly would
    // admit a value that then truncates to a nonsense `usize`. Compare
    // against a power of two that survives the conversion exactly.
    if bytes >= 9_007_199_254_740_992.0 {
        return Err(bad());
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let bytes = bytes as usize;
    Ok(Some(bytes))
}

/// Render a byte size back in the friendliest exact form.
///
/// Public alongside [`parse_byte_size`] and for the same reason: a shell
/// showing the current ceiling should show it in the spelling the file
/// uses, so what the operator reads in a settings window and what they
/// read in `settings.txt` are the same string.
///
/// Exactness first: a value that is a whole number of MiB or GiB is
/// written that way, and anything else is written as bytes. The round trip
/// through [`parse_byte_size`] is therefore lossless for every value,
/// which is what the settings round-trip test demands — a "friendly"
/// writer that rounded would silently change the operator's setting every
/// time pdfcer saved the file.
#[must_use]
pub fn format_byte_size(bytes: Option<usize>) -> String {
    const KIB: usize = 1024;
    const MIB: usize = 1024 * KIB;
    const GIB: usize = 1024 * MIB;
    match bytes {
        None => "default".to_owned(),
        Some(0) => "0".to_owned(),
        Some(b) if b % GIB == 0 => format!("{}gib", b / GIB),
        Some(b) if b % MIB == 0 => format!("{}mib", b / MIB),
        Some(b) => b.to_string(),
    }
}

/// Lowest accepted `word_gap_ratio`. Zero would break a word at every
/// glyph pair.
///
/// Public so a front end can bound its own control by the **same** number
/// the parser clamps to. A slider whose range is a restated literal is a
/// slider that eventually disagrees with the file's own validation, and
/// then the operator drags to a value that silently clamps.
pub const MIN_WORD_GAP_RATIO: f32 = 0.01;
/// Highest accepted `word_gap_ratio`. Beyond this a line never breaks
/// into words at all. Public for the same reason as
/// [`MIN_WORD_GAP_RATIO`].
pub const MAX_WORD_GAP_RATIO: f32 = 5.0;

/// Lowest accepted `parallel_epsilon_degrees`.
///
/// Zero is allowed and means "exactly parallel only" — a legitimate strict
/// choice for someone working with exact CAD output, not a degenerate value,
/// so it is the floor rather than being rejected.
pub const MIN_PARALLEL_EPSILON_DEGREES: f64 = 0.0;
/// Highest accepted `parallel_epsilon_degrees`.
///
/// Above 45 degrees the classification inverts in spirit: more pairs would be
/// called parallel than angled, which is no longer a tolerance on "parallel"
/// but a different feature. Public for the same reason as
/// [`MIN_WORD_GAP_RATIO`] — a front end bounds its control by THIS number
/// rather than a restated literal.
pub const MAX_PARALLEL_EPSILON_DEGREES: f64 = 45.0;

/// The settings-file token for a separation policy.
///
/// Defined once and used by both [`Settings::apply`] (to say what it fell
/// back to) and [`Settings::write_to_string`] (to write it out). Spelling
/// a token in two places is how a writer and a parser come to disagree
/// about the same value — the same failure the `word_gap_ratio` default
/// already demonstrated in this module.
const fn separation_token(policy: SeparationPolicy) -> &'static str {
    match policy {
        SeparationPolicy::Repair => "repair",
        SeparationPolicy::Discard => "discard",
        SeparationPolicy::Refuse => "refuse",
    }
}

/// The settings-file token for a CMYK intent. See [`separation_token`].
/// The persisted spelling of a [`StylePolicy`], for the settings file and for
/// the `BadValue` note that names the fallback.
const fn style_policy_token(policy: StylePolicy) -> &'static str {
    match policy {
        StylePolicy::Auto => "auto",
        StylePolicy::Warn => "warn",
        StylePolicy::Refuse => "refuse",
    }
}

const fn cmyk_token(intent: CmykIntent) -> &'static str {
    match intent {
        CmykIntent::Calibrated => "calibrated",
        CmykIntent::NeutralBlack => "neutral_black",
    }
}

/// The settings-file token for a page-blend-space source. See
/// [`separation_token`] for why every enum gets one of these.
const fn page_blend_space_source_token(src: PageBlendSpaceSource) -> &'static str {
    match src {
        PageBlendSpaceSource::DeviceNative => "device_native",
        PageBlendSpaceSource::OutputIntentIfSubtractive => "output_intent_if_subtractive",
        PageBlendSpaceSource::OutputIntentAlways => "output_intent_always",
    }
}

/// The settings-file token for a mesh patch-padding reading. See
/// [`separation_token`] for why every enum gets one of these.
const fn mesh_patch_padding_token(p: MeshPatchPadding) -> &'static str {
    match p {
        MeshPatchPadding::PerRecord => "per_record",
        MeshPatchPadding::None => "none",
    }
}

/// The settings-file token for a mask resampling filter. See
/// [`separation_token`] for why every enum gets one of these.
const fn mask_resample_token(filter: MaskResample) -> &'static str {
    match filter {
        MaskResample::Nearest => "nearest",
        MaskResample::BoxAverage => "box_average",
        MaskResample::Bilinear => "bilinear",
    }
}

/// The settings-file token for a minification filter. See
/// [`separation_token`].
const fn minify_token(filter: MinifyFilter) -> &'static str {
    match filter {
        MinifyFilter::PointSample => "point_sample",
        MinifyFilter::Smooth => "smooth",
    }
}

/// The settings-file token for a CMYK-JPEG polarity rule. See
/// [`separation_token`].
const fn cmyk_jpeg_polarity_token(polarity: CmykJpegPolarity) -> &'static str {
    match polarity {
        CmykJpegPolarity::NeverInvert => "never_invert",
        CmykJpegPolarity::InvertOnApp14 => "invert_on_app14",
    }
}

/// The settings-file token for an unmappable-code sentinel. See
/// [`separation_token`].
const fn unmappable_token(sentinel: UnmappableCode) -> &'static str {
    match sentinel {
        UnmappableCode::ReplacementChar => "replacement_char",
        UnmappableCode::QuestionMark => "question_mark",
        UnmappableCode::Omit => "omit",
    }
}

/// The settings-file token for an `/ActualText` precedence rule. See
/// [`separation_token`].
const fn actual_text_token(precedence: ActualTextPrecedence) -> &'static str {
    match precedence {
        ActualTextPrecedence::Always => "always",
        ActualTextPrecedence::TaggedOnly => "tagged_only",
        ActualTextPrecedence::Glyphs => "glyphs",
    }
}

/// The settings-file token for a missing-`/AS` policy. See
/// [`separation_token`].
const fn missing_as_token(policy: MissingAppearanceState) -> &'static str {
    match policy {
        MissingAppearanceState::PaintNothing => "paint_nothing",
        MissingAppearanceState::FirstEntry => "first_entry",
        MissingAppearanceState::OffElseNothing => "off_else_nothing",
    }
}

/// The settings-file token for a cross-reference entry terminator. See
/// [`separation_token`].
/// The settings-file token for a [`QuadPointOrder`].
const fn quad_point_order_token(order: QuadPointOrder) -> &'static str {
    match order {
        QuadPointOrder::ReadingOrder => "reading_order",
        QuadPointOrder::Counterclockwise => "counterclockwise",
    }
}
const fn xref_entry_eol_token(eol: XrefEntryEol) -> &'static str {
    match eol {
        XrefEntryEol::MatchSource => "match_source",
        XrefEntryEol::SpaceLf => "space_lf",
        XrefEntryEol::SpaceCr => "space_cr",
        XrefEntryEol::CrLf => "cr_lf",
    }
}

/// The settings-file token for the trailing-EOL rule. See
/// [`separation_token`].
const fn trailing_eol_token(eol: TrailingEol) -> &'static str {
    match eol {
        TrailingEol::Lf => "lf",
        TrailingEol::None => "none",
    }
}

impl Settings {
    /// Load the operator's settings, always successfully.
    ///
    /// Reads from `location`. A missing file, an unreadable one, or a file
    /// full of nonsense all yield usable settings; what went wrong is in
    /// the returned [`LoadReport`]. See the module docs' fail-soft table.
    #[must_use]
    pub fn load(location: StoreLocation) -> (Self, LoadReport) {
        let mut report = LoadReport {
            location,
            existed: false,
            notes: Vec::new(),
        };
        let Some(path) = report.location.path.clone() else {
            return (Self::default(), report);
        };
        if !path.exists() {
            // A first run is the expected state, not a fault.
            return (Self::default(), report);
        }
        report.existed = true;
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => {
                report.notes.push(SettingNote::Unreadable {
                    path,
                    reason: error.to_string(),
                });
                return (Self::default(), report);
            }
        };
        let settings = Self::parse(&text, &mut report.notes);
        (settings, report)
    }

    /// Parse settings text, recovering per key.
    ///
    /// Split out from [`Settings::load`] so the whole grammar is testable
    /// without a filesystem — which is also what lets the fail-soft table
    /// in the module docs be pinned by tests rather than merely asserted
    /// in prose.
    #[must_use]
    pub fn parse(text: &str, notes: &mut Vec<SettingNote>) -> Self {
        let mut settings = Self::default();
        let mut seen: Vec<String> = Vec::new();

        for (index, raw) in text.lines().enumerate() {
            let line = index + 1;
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                notes.push(SettingNote::Malformed { line });
                continue;
            };
            let key = key.trim().to_owned();
            let value = value.trim();
            if key.is_empty() {
                notes.push(SettingNote::Malformed { line });
                continue;
            }
            if seen.contains(&key) {
                notes.push(SettingNote::Duplicate {
                    key: key.clone(),
                    line,
                });
            } else {
                seen.push(key.clone());
            }
            settings.apply(&key, value, line, notes);
        }
        settings
    }

    /// Apply one `key = value` pair, noting anything that did not take.
    ///
    /// The one place that knows the file's vocabulary. Every arm either
    /// sets a field or pushes a note — an arm that does neither would be a
    /// setting that silently does nothing, which is the failure this
    /// module exists to prevent.
    fn apply(&mut self, key: &str, value: &str, line: usize, notes: &mut Vec<SettingNote>) {
        match key {
            "separations" => match value {
                "repair" => self.separations = SeparationPolicy::Repair,
                "discard" => self.separations = SeparationPolicy::Discard,
                "refuse" => self.separations = SeparationPolicy::Refuse,
                _ => notes.push(SettingNote::BadValue {
                    key: key.to_owned(),
                    value: value.to_owned(),
                    line,
                    using: separation_token(Self::default().separations).to_owned(),
                }),
            },
            "style_policy" => match value {
                "auto" => self.style_policy = StylePolicy::Auto,
                "warn" => self.style_policy = StylePolicy::Warn,
                "refuse" => self.style_policy = StylePolicy::Refuse,
                _ => notes.push(SettingNote::BadValue {
                    key: key.to_owned(),
                    value: value.to_owned(),
                    line,
                    using: style_policy_token(Self::default().style_policy).to_owned(),
                }),
            },
            "cmyk_intent" => match value {
                "calibrated" => self.cmyk_intent = CmykIntent::Calibrated,
                "neutral_black" => self.cmyk_intent = CmykIntent::NeutralBlack,
                _ => notes.push(SettingNote::BadValue {
                    key: key.to_owned(),
                    value: value.to_owned(),
                    line,
                    using: cmyk_token(Self::default().cmyk_intent).to_owned(),
                }),
            },
            "max_cmyk_buffer_bytes" => match parse_byte_size(value) {
                Ok(parsed) => self.max_cmyk_buffer_bytes = parsed,
                Err(_) => notes.push(SettingNote::BadValue {
                    key: key.to_owned(),
                    value: value.to_owned(),
                    line,
                    using: "default".to_owned(),
                }),
            },
            // Unvalidated on purpose — see the field docs.
            "theme" => self.theme = value.to_owned(),
            "parallel_epsilon_degrees" => match value.parse::<f64>() {
                Ok(parsed) if parsed.is_finite() => {
                    let clamped =
                        parsed.clamp(MIN_PARALLEL_EPSILON_DEGREES, MAX_PARALLEL_EPSILON_DEGREES);
                    if (clamped - parsed).abs() > f64::EPSILON {
                        notes.push(SettingNote::Clamped {
                            key: key.to_owned(),
                            value: value.to_owned(),
                            line,
                            using: clamped.to_string(),
                        });
                    }
                    self.parallel_epsilon_degrees = clamped;
                }
                _ => notes.push(SettingNote::BadValue {
                    key: key.to_owned(),
                    value: value.to_owned(),
                    line,
                    using: Self::default().parallel_epsilon_degrees.to_string(),
                }),
            },
            "word_gap_ratio" => match value.parse::<f32>() {
                Ok(parsed) if parsed.is_finite() => {
                    let clamped = parsed.clamp(MIN_WORD_GAP_RATIO, MAX_WORD_GAP_RATIO);
                    // `!=` on floats is exactly right here: the question is
                    // whether `clamp` returned a different number, not
                    // whether two computed values are near each other.
                    if (clamped - parsed).abs() > f32::EPSILON {
                        notes.push(SettingNote::Clamped {
                            key: key.to_owned(),
                            value: value.to_owned(),
                            line,
                            using: clamped.to_string(),
                        });
                    }
                    self.word_gap_ratio = clamped;
                }
                _ => notes.push(SettingNote::BadValue {
                    key: key.to_owned(),
                    value: value.to_owned(),
                    line,
                    using: Self::default().word_gap_ratio.to_string(),
                }),
            },
            "page_blend_space_source" => match value {
                "device_native" => {
                    self.page_blend_space_source = PageBlendSpaceSource::DeviceNative;
                }
                "output_intent_if_subtractive" => {
                    self.page_blend_space_source = PageBlendSpaceSource::OutputIntentIfSubtractive;
                }
                "output_intent_always" => {
                    self.page_blend_space_source = PageBlendSpaceSource::OutputIntentAlways;
                }
                _ => notes.push(SettingNote::BadValue {
                    key: key.to_owned(),
                    value: value.to_owned(),
                    line,
                    using: page_blend_space_source_token(Self::default().page_blend_space_source)
                        .to_owned(),
                }),
            },
            "spot_colorant_device_model" => match SpotColorantDeviceModel::parse(value) {
                Some(model) => self.spot_colorant_device_model = model,
                // A bad value is a NOTE and the default stands, exactly as
                // every neighbouring arm does it -- a settings file with one
                // typo must still load, and the operator must be told which
                // value is actually in force rather than left to infer it.
                None => notes.push(SettingNote::BadValue {
                    key: key.to_owned(),
                    value: value.to_owned(),
                    line,
                    using: Self::default()
                        .spot_colorant_device_model
                        .as_str()
                        .to_owned(),
                }),
            },
            "overprint_zero_tint_scope" => match OverprintZeroTintScope::parse(value) {
                Some(scope) => self.overprint_zero_tint_scope = scope,
                None => notes.push(SettingNote::BadValue {
                    key: key.to_owned(),
                    value: value.to_owned(),
                    line,
                    using: Self::default()
                        .overprint_zero_tint_scope
                        .as_str()
                        .to_owned(),
                }),
            },
            "mesh_patch_padding" => match value {
                "per_record" => self.mesh_patch_padding = MeshPatchPadding::PerRecord,
                "none" => self.mesh_patch_padding = MeshPatchPadding::None,
                _ => notes.push(SettingNote::BadValue {
                    key: key.to_owned(),
                    value: value.to_owned(),
                    line,
                    using: mesh_patch_padding_token(Self::default().mesh_patch_padding).to_owned(),
                }),
            },
            "mask_resample" => match value {
                "nearest" => self.mask_resample = MaskResample::Nearest,
                "box_average" => self.mask_resample = MaskResample::BoxAverage,
                "bilinear" => self.mask_resample = MaskResample::Bilinear,
                _ => notes.push(SettingNote::BadValue {
                    key: key.to_owned(),
                    value: value.to_owned(),
                    line,
                    using: mask_resample_token(Self::default().mask_resample).to_owned(),
                }),
            },
            "image_minify" => match value {
                "point_sample" => self.image_minify = MinifyFilter::PointSample,
                "smooth" => self.image_minify = MinifyFilter::Smooth,
                _ => notes.push(SettingNote::BadValue {
                    key: key.to_owned(),
                    value: value.to_owned(),
                    line,
                    using: minify_token(Self::default().image_minify).to_owned(),
                }),
            },
            "cmyk_jpeg_polarity" => match value {
                "never_invert" => self.cmyk_jpeg_polarity = CmykJpegPolarity::NeverInvert,
                "invert_on_app14" => self.cmyk_jpeg_polarity = CmykJpegPolarity::InvertOnApp14,
                _ => notes.push(SettingNote::BadValue {
                    key: key.to_owned(),
                    value: value.to_owned(),
                    line,
                    using: cmyk_jpeg_polarity_token(Self::default().cmyk_jpeg_polarity).to_owned(),
                }),
            },
            "unmappable_code" => match value {
                "replacement_char" => self.unmappable_code = UnmappableCode::ReplacementChar,
                "question_mark" => self.unmappable_code = UnmappableCode::QuestionMark,
                "omit" => self.unmappable_code = UnmappableCode::Omit,
                _ => notes.push(SettingNote::BadValue {
                    key: key.to_owned(),
                    value: value.to_owned(),
                    line,
                    using: unmappable_token(Self::default().unmappable_code).to_owned(),
                }),
            },
            "actual_text" => match value {
                "always" => self.actual_text = ActualTextPrecedence::Always,
                "tagged_only" => self.actual_text = ActualTextPrecedence::TaggedOnly,
                "glyphs" => self.actual_text = ActualTextPrecedence::Glyphs,
                _ => notes.push(SettingNote::BadValue {
                    key: key.to_owned(),
                    value: value.to_owned(),
                    line,
                    using: actual_text_token(Self::default().actual_text).to_owned(),
                }),
            },
            "missing_as" => match value {
                "paint_nothing" => self.missing_as = MissingAppearanceState::PaintNothing,
                "first_entry" => self.missing_as = MissingAppearanceState::FirstEntry,
                "off_else_nothing" => self.missing_as = MissingAppearanceState::OffElseNothing,
                _ => notes.push(SettingNote::BadValue {
                    key: key.to_owned(),
                    value: value.to_owned(),
                    line,
                    using: missing_as_token(Self::default().missing_as).to_owned(),
                }),
            },
            "quad_point_order" => match value {
                "reading_order" => self.quad_point_order = QuadPointOrder::ReadingOrder,
                "counterclockwise" => {
                    self.quad_point_order = QuadPointOrder::Counterclockwise;
                }
                _ => notes.push(SettingNote::BadValue {
                    key: key.to_owned(),
                    value: value.to_owned(),
                    line,
                    using: quad_point_order_token(self.quad_point_order).to_owned(),
                }),
            },
            "xref_entry_eol" => match value {
                "match_source" => self.xref_entry_eol = XrefEntryEol::MatchSource,
                "space_lf" => self.xref_entry_eol = XrefEntryEol::SpaceLf,
                "space_cr" => self.xref_entry_eol = XrefEntryEol::SpaceCr,
                "cr_lf" => self.xref_entry_eol = XrefEntryEol::CrLf,
                _ => notes.push(SettingNote::BadValue {
                    key: key.to_owned(),
                    value: value.to_owned(),
                    line,
                    using: xref_entry_eol_token(Self::default().xref_entry_eol).to_owned(),
                }),
            },
            "trailing_eol" => match value {
                "lf" => self.trailing_eol = TrailingEol::Lf,
                "none" => self.trailing_eol = TrailingEol::None,
                _ => notes.push(SettingNote::BadValue {
                    key: key.to_owned(),
                    value: value.to_owned(),
                    line,
                    using: trailing_eol_token(Self::default().trailing_eol).to_owned(),
                }),
            },
            _ => notes.push(SettingNote::UnknownKey {
                key: key.to_owned(),
                line,
            }),
        }
    }

    /// Render the settings as the file's text, with explanatory comments.
    ///
    /// The comments are not decoration: this file is meant to be opened in
    /// a text editor, and a bare `cmyk_intent = calibrated` tells an
    /// operator nothing about what the alternatives are or what flipping it
    /// would change. Every key therefore carries its legal values.
    #[must_use]
    pub fn write_to_string(&self) -> String {
        let mut out = String::new();
        out.push_str(
            "# pdfcer settings\n\
             #\n\
             # Plain text, one `key = value` per line. Lines starting with # are\n\
             # comments. An unknown key is ignored and reported, not deleted, and a\n\
             # value pdfcer cannot read falls back to the default for that key alone —\n\
             # one bad line never discards the rest of the file.\n\
             #\n\
             # KEEP THIS FOLDER when you update pdfcer. Updating means replacing the\n\
             # program files, and everything in this folder is yours, not the\n\
             # program's.\n\n",
        );

        out.push_str(
            "# What to do when a page operation splits a preseparated page set —\n\
             # a print-ready file where one logical page is several page objects,\n\
             # one per printing plate (ISO 32000-1 section 14.11.4).\n\
             #   repair  = keep the surviving plates and update them (default)\n\
             #   discard = keep the pages, forget they were separations\n\
             #   refuse  = decline the operation instead\n",
        );
        let _ = writeln!(
            out,
            "separations = {}\n",
            match self.separations {
                SeparationPolicy::Repair => "repair",
                SeparationPolicy::Discard => "discard",
                SeparationPolicy::Refuse => "refuse",
            }
        );

        out.push_str(
            "# How CMYK colour is converted for display. The PDF standard defines no\n\
             # conversion at all (section 8.6.4.4), so this is a choice, not a fact.\n\
             #   calibrated    = match how Acrobat and most viewers render it (default).\n\
             #                   Solid black ink shows as a very dark warm grey, not\n\
             #                   pure black, because that is what those viewers do.\n\
             #   neutral_black = pure black ink renders true black. Right for CAD and\n\
             #                   line drawings, where every line is stroked in pure K.\n\
             #                   Only pure black differs; every mixed colour is still\n\
             #                   the calibrated one.\n",
        );
        let _ = writeln!(
            out,
            "cmyk_intent = {}\n",
            match self.cmyk_intent {
                CmykIntent::Calibrated => "calibrated",
                CmykIntent::NeutralBlack => "neutral_black",
            }
        );

        out.push_str(
            "# What pdfcer does when you make text BOLD or ITALIC and the real\n\
             # face may not be there.\n\
             #\n\
             # Bold is not a switch in a PDF -- it is a different typeface. So\n\
             # pdfcer looks for a real bold face first and thickens the letters\n\
             # only if it cannot find one. That ladder is the same whatever you\n\
             # set here; this only decides what pdfcer DOES about having had to\n\
             # fall back.\n\
             #   auto   = just do it, and say afterwards which it used (default).\n\
             #            You are never asked and never stopped.\n\
             #   warn   = the same, but say so loudly when the weight was faked\n\
             #            rather than real. For work where a fake bold in the\n\
             #            output is worth noticing as it happens.\n\
             #   refuse = if you specifically ask to fake it and a real face was\n\
             #            available, stop and name that face instead. Strict, and\n\
             #            what pdfcer did before this was a choice.\n\
             #\n\
             # Naming a font yourself, or forcing the fake one, always works and\n\
             # is not affected by this.\n",
        );
        let _ = writeln!(
            out,
            "style_policy = {}\n",
            style_policy_token(self.style_policy)
        );

        out.push_str(
            "# How much memory pdfcer may use to blend a page in PRINT COLOURS.\n\
             #\n\
             # A page that declares a CMYK blending space is blended in ink rather\n\
             # than on screen, which needs 20 bytes per pixel — so the memory grows\n\
             # with the SQUARE of the zoom. Above this ceiling pdfcer blends on screen\n\
             # instead, says so, and the colours become approximate. That is why the\n\
             # same page can look slightly different at different zoom levels.\n\
             #\n\
             # Raise it to keep exact colours further in. It costs memory, and about\n\
             # 50% more time on the pages it applies to. There is NO UPPER LIMIT —\n\
             # it is your machine and your call. A value your machine cannot supply\n\
             # is not a crash: pdfcer falls back and tells you.\n\
             #   default = pdfcer's built-in ceiling\n\
             #   256mib, 1gib, 2gb, 268435456 = all accepted; mb and mib both mean\n\
             #                                  1,048,576 bytes here\n\
             #   0       = never blend in print colours at all\n\
             # Rough guide, whole A4 page: 256mib reaches about 518% zoom, 1gib\n\
             # about 1035%, 4gib about the largest page pdfcer will raster at all.\n\
             # A page with layered transparency can need up to about FOUR TIMES\n\
             # this at once, because each layer is given a buffer of its own.\n",
        );
        let _ = writeln!(
            out,
            "max_cmyk_buffer_bytes = {}\n",
            format_byte_size(self.max_cmyk_buffer_bytes)
        );

        out.push_str(
            "# How far apart two glyphs must be, as a multiple of the font size,\n\
             # before extracted text gets a space between them. Raise it if\n\
             # extraction is splitting words; lower it if it is running them\n\
             # together. Accepted range 0.01 to 5.0.\n",
        );
        let _ = writeln!(out, "word_gap_ratio = {}\n", self.word_gap_ratio);

        out.push_str(
            "# When you dimension between two lines, how many degrees apart they may\n\
             # be and still be treated as parallel (giving a distance) rather than\n\
             # as an angle. Nothing in any standard fixes this, so it is yours to\n\
             # set: exported CAD geometry is usually exact, so a small value avoids\n\
             # calling a rounding artefact a taper. 0 means exactly parallel only.\n\
             # You can always force the parallel reading on one dimension without\n\
             # changing this. Accepted range 0 to 45.\n",
        );
        let _ = writeln!(
            out,
            "parallel_epsilon_degrees = {}\n",
            self.parallel_epsilon_degrees
        );

        out.push_str(
            "# When a picture carries a separate transparency image at a different\n\
             # size, this decides how that transparency is stretched to fit. The PDF\n\
             # standard fixes where the two line up and says nothing about how to\n\
             # stretch (section 8.9.6.3).\n\
             #   nearest     = take the single nearest transparency pixel (default).\n\
             #                 Keeps hard cut-out edges perfectly sharp and can never\n\
             #                 invent a half-transparent pixel that was not there.\n\
             #                 Can look stair-stepped.\n\
             #   box_average = average every transparency pixel the picture pixel\n\
             #                 covers. Best when the transparency is FINER than the\n\
             #                 picture, where `nearest` throws most of it away.\n\
             #   bilinear    = blend smoothly between transparency pixels. Best for a\n\
             #                 soft photographic fade supplied coarser than the\n\
             #                 picture; softens hard cut-out edges, which is usually\n\
             #                 not wanted.\n",
        );
        let _ = writeln!(
            out,
            "mask_resample = {}\n",
            mask_resample_token(self.mask_resample)
        );

        out.push_str(
            "# Where a page's BLENDING COLOUR SPACE comes from when the page group\n\
             # does not declare one. This decides whether OVERPRINT can work at all.\n\
             #\n\
             # ISO 32000-1 is determinate here -- the device's native space, which\n\
             # for pdfcer means sRGB -- and in sRGB overprint is not merely\n\
             # approximated, it is UNREPRESENTABLE. ISO 32000-2's Annex P allows the\n\
             # output intent to supply the space instead, but says so informatively\n\
             # and without ranking the two, so this is a genuine choice rather than a\n\
             # right answer and a wrong one.\n\
             #\n\
             #   device_native                 ISO 32000-1 to the letter. Reproduces\n\
             #                                 pdfcer's output before this setting\n\
             #                                 existed. Overprint renders degenerately.\n\
             #   output_intent_if_subtractive  Use the output intent's space when it is\n\
             #                                 a four-or-more-colorant one. A PDF/X\n\
             #                                 print file then renders the way a\n\
             #                                 print-oriented viewer renders it. An RGB\n\
             #                                 or grey output intent changes nothing.\n\
             #   output_intent_always          Annex P read literally: any output\n\
             #                                 intent supplies the space.\n\
             #\n\
             # Whichever is chosen, the space pdfcer actually used and WHERE IT CAME\n\
             # FROM are reported on `pdfcer render-page`'s metrics line. Nothing is\n\
             # drawn on the page.\n",
        );
        let _ = writeln!(
            out,
            "page_blend_space_source = {}\n",
            page_blend_space_source_token(self.page_blend_space_source)
        );

        out.push_str(
            "# Which colour spaces get OPM 1's zero-tint rule under overprint.\n\
             # ISO 32000-1 8.6.7 scopes that rule to a DeviceCMYK source, and its one\n\
             # escape hatch points at 8.6.5.7, which covers CIE-BASED spaces only. So a\n\
             # DeviceGray fill overprinting a spot backdrop either knocks it out (the\n\
             # literal reading) or preserves it (converting grey to K-only CMYK first,\n\
             # then applying the rule). That is a DIVERGENCE, not a spec ambiguity:\n\
             # 11.7.4.5 Table 149 puts a DeviceGray source in the process-space row.\n\
             #\n\
             #   device_cmyk_only   DEFAULT (since v0.25.0). 8.6.7 to the letter: only a\n\
             #                      DeviceCMYK source skips its zero tints. Matches the\n\
             #                      reference render on every patch of the print-\n\
             #                      conformance suite now that pdfcer keeps spot inks on\n\
             #                      their own plane, so a grey over a spot still\n\
             #                      preserves it under this value.\n\
             #   grey_as_k_only     The default up to v0.24.0. Treats DeviceGray as the\n\
             #                      K-only CMYK it converts to, so its zero C, M and Y\n\
             #                      preserve a PROCESS backdrop where the standard and\n\
             #                      the reference replace it.\n\
             #   all_process_spaces also DeviceRGB and CalRGB. Principled but\n\
             #                      unmeasured -- no corpus patch exercises it\n\
             #\n\
             # A sampled image is never upgraded under any value: Table 149 already\n\
             # excludes a CMYK image from the direct-CMYK row, and a grey image is that\n\
             # case's analogue.\n",
        );
        let _ = writeln!(
            out,
            "overprint_zero_tint_scope = {}\n",
            self.overprint_zero_tint_scope.as_str()
        );

        out.push_str("# Which OUTPUT DEVICE a spot colorant is rendered for -- spec fork OP-A7.\n");
        out.push_str(
            "# ISO 32000-1 8.6.6.4 REQUIRES substituting a Separation's alternate colour\n",
        );
        out.push_str(
            "# space when the device has no colorant of that name, which a screen never\n",
        );
        out.push_str("# does. ISO 32000-2 10.8.3 PERMITS simulating a device that does. Both\n");
        out.push_str("# conform, and they differ: a white object overprinting a spot backdrop\n");
        out.push_str("# knocks it out under the first and preserves it under the second.\n");
        out.push_str("#\n");
        out.push_str("#   simulate_separations          the default -- keep the ink on its own\n"); // string-gap-exempt: a two-column layout in the settings file itself, aligning each option token with its description; the run of spaces IS the column
        out.push_str("#                                 plate; 8.6.6.4 NOTE 7 and 10.8.3 both\n"); // string-gap-exempt: a two-column layout in the settings file itself, aligning each option token with its description; the run of spaces IS the column
        out.push_str("#                                 rank this better under overprint\n"); // string-gap-exempt: a two-column layout in the settings file itself, aligning each option token with its description; the run of spaces IS the column
        out.push_str("#   alternate_space_substitution  render for the actual composite device,\n"); // string-gap-exempt: a two-column layout in the settings file itself, aligning each option token with its description; the run of spaces IS the column
        out.push_str("#                                 reproducing what a screen viewer shows\n"); // string-gap-exempt: a two-column layout in the settings file itself, aligning each option token with its description; the run of spaces IS the column
        out.push_str("#\n");
        out.push_str(
            "# Measured on the print-conformance corpus: the default trips 9 trap marks\n",
        );
        out.push_str(
            "# across the four spot patches, the alternative 16 -- those traps exist to\n",
        );
        out.push_str("# catch a composite renderer, so the corpus expects the default.\n");
        let _ = writeln!(
            out,
            "spot_colorant_device_model = {}\n",
            self.spot_colorant_device_model.as_str()
        );

        out.push_str(
            "# How a mesh shading PATCH record (shading types 6 and 7) is padded.\n\
             # ISO 32000-1 states that rule for a VERTEX, and a patch has no vertices;\n\
             # the patch clauses point back at it without saying what its unit becomes.\n\
             # ISO 32000-2 repeats the sentence word for word, so this is permanently\n\
             # ambiguous rather than merely unresearched.\n\
             #   per_record  pad each patch record to a whole byte (the default)\n\
             #   none        read the patch stream as one continuous bit string\n\
             #\n\
             # It changes nothing unless a file's BitsPerFlag/BitsPerCoordinate/\n\
             # BitsPerComponent make a record a non-multiple of 8 bits. Every mesh\n\
             # measured so far is byte aligned either way.\n",
        );
        let _ = writeln!(
            out,
            "mesh_patch_padding = {}\n",
            mesh_patch_padding_token(self.mesh_patch_padding)
        );

        out.push_str(
            "# Which look the window uses. The application's own colours and spacing\n\
             # ONLY -- it never changes a document, and nothing here is written into\n\
             # a PDF you save.\n\
             #   quiet = muted greys, one accent, tight spacing (the default)\n\
             #   airy  = lighter, roomier, softer edges\n\
             #   dark  = a dark window against a light page, as CAD tools do it\n\
             # An unrecognised name is reported when pdfcer starts and the default is\n\
             # used for that run; the name you wrote is kept, not overwritten.\n",
        );
        let _ = writeln!(out, "theme = {}\n", self.theme);

        out.push_str(
            "# How a picture is drawn when it is shown SMALLER than its own pixel\n\
             # grid. The standard only describes smoothing for making a picture\n\
             # bigger and never mentions making one smaller (section 8.9.5.3), so\n\
             # this direction is pdfcer's choice.\n\
             #   point_sample = take one pixel per dot on screen (default). Exact, and\n\
             #                  what the document's own smoothing switch literally\n\
             #                  asks for; thin lines can shimmer or vanish when the\n\
             #                  picture is shrunk a lot.\n\
             #   smooth       = average when shrinking. Cleaner shrunken photographs;\n\
             #                  a deliberate departure from the document's switch.\n",
        );
        let _ = writeln!(out, "image_minify = {}\n", minify_token(self.image_minify));

        out.push_str(
            "# How to read a four-ink (CMYK) JPEG that does not say which way round\n\
             # its ink values are stored. No document anywhere defines this; some\n\
             # 1990s Photoshop output stores the values back-to-front and says so\n\
             # nowhere. Getting it wrong turns the picture into a photographic\n\
             # negative, so the mistake is at least obvious.\n\
             #   never_invert    = take the values as stored (default). What every\n\
             #                     other major PDF reader does; a document can still\n\
             #                     declare inverted storage the proper way, and pdfcer\n\
             #                     honours that.\n\
             #   invert_on_app14 = flip the values when the file carries an Adobe\n\
             #                     marker and declares nothing. Only for a library of\n\
             #                     old Photoshop CMYK JPEGs that really are stored\n\
             #                     back-to-front.\n",
        );
        let _ = writeln!(
            out,
            "cmyk_jpeg_polarity = {}\n",
            cmyk_jpeg_polarity_token(self.cmyk_jpeg_polarity)
        );

        out.push_str(
            "# What copied or searched text shows for a character pdfcer cannot read\n\
             # at all — a font that carries no way back to real characters. The\n\
             # standard names no stand-in (section 9.10.2). CHANGING THIS CHANGES\n\
             # WHICH TEXT A SEARCH OR A TEXT-BASED REDACTION MATCHES.\n\
             #   replacement_char = the standard black-diamond question mark, one per\n\
             #                      unreadable character (default). Keeps the text the\n\
             #                      same length and is unmistakably a failure.\n\
             #   question_mark    = a plain ? instead. Survives being pasted anywhere,\n\
             #                      but is indistinguishable from a real ? in the\n\
             #                      document.\n\
             #   omit             = show nothing. The text gets shorter with no sign\n\
             #                      in the text that anything was lost, and a line\n\
             #                      whose characters are ALL unreadable disappears\n\
             #                      from the results entirely. pdfcer still counts\n\
             #                      every such character whichever setting you use.\n",
        );
        let _ = writeln!(
            out,
            "unmappable_code = {}\n",
            unmappable_token(self.unmappable_code)
        );

        out.push_str(
            "# Some documents attach a \"what this really says\" note to a piece of\n\
             # text — for a ligature, a logo, or an abbreviation. This decides\n\
             # whether that note replaces what is drawn on the page. The standard\n\
             # says one thing in section 14.9.4 and something else in a note to\n\
             # section 14.8, so both readings are defensible.\n\
             #   always      = the note wins wherever it appears (default).\n\
             #   tagged_only = the note wins only inside properly tagged content, and\n\
             #                 the drawn characters win everywhere else. Use it when a\n\
             #                 producer scatters bad notes outside its tagging.\n\
             #   glyphs      = the drawn characters always win; the note is reported\n\
             #                 but never substituted. Use it when you need what the\n\
             #                 page actually shows. Text whose ONLY real identity was\n\
             #                 in the note becomes unreadable.\n",
        );
        let _ = writeln!(
            out,
            "actual_text = {}\n",
            actual_text_token(self.actual_text)
        );

        out.push_str(
            "# What to show for a stamp, checkbox or other marked-up item that\n\
             # supplies several alternative appearances but forgets to say which one\n\
             # is current. Such a file is malformed and the standard states no\n\
             # recovery (section 12.5.5). pdfcer never repairs the file; it only\n\
             # decides what to put on screen, and counts every occurrence.\n\
             #   paint_nothing    = show nothing, and report it (default). The honest\n\
             #                      answer: pdfcer will not pick one for you.\n\
             #   first_entry      = show the first alternative the file lists.\n\
             #   off_else_nothing = show the \"off\" alternative if there is one,\n\
             #                      otherwise nothing. The checkbox-shaped guess.\n",
        );
        let _ = writeln!(out, "missing_as = {}\n", missing_as_token(self.missing_as));

        out.push_str(
            "# Two invisible bookkeeping bytes at the end of every line of a saved\n\
             # file's index table. The standard permits exactly these three and no\n\
             # others (section 7.5.4). Nothing on screen changes; only the saved\n\
             # bytes do.\n\
             #\n\
             # The default keeps whatever form the file you opened already used,\n\
             # so saving a document pdfcer did not otherwise change does not\n\
             # rewrite two bytes in every line of its index for no reason.\n\
             #   match_source = keep the form the file already uses (default);\n\
             #                  space_lf for a file that has none\n\
             #   space_lf     = always space then line-feed\n\
             #   space_cr     = always space then carriage-return\n\
             #   cr_lf        = always carriage-return then line-feed\n",
        );
        let _ = writeln!(
            out,
            "xref_entry_eol = {}\n",
            xref_entry_eol_token(self.xref_entry_eol)
        );

        out.push_str(
            "# Corner order for the /QuadPoints array pdfcer writes on highlight,\n\
             # underline, strike-out, squiggly and redaction annotations.\n\
             #\n\
             # The standard describes a counterclockwise walk; Acrobat, PDFBox and\n\
             # pdf.js all emit and expect reading order instead. pdfcer bakes a full\n\
             # appearance, so this never affects how pdfcer itself draws the mark --\n\
             # only how another tool reads the geometry back. The wrong order there\n\
             # describes a bow-tie instead of a rectangle.\n\
             #   reading_order    = upper-left, upper-right, lower-left, lower-right\n\
             #                      (default). What the readers most files are opened\n\
             #                      in expect.\n\
             #   counterclockwise = upper-left, upper-right, lower-right, lower-left.\n\
             #                      The letter of the standard, for output going to a\n\
             #                      conformance checker.\n",
        );
        let _ = writeln!(
            out,
            "quad_point_order = {}\n",
            quad_point_order_token(self.quad_point_order)
        );
        out.push_str(
            "# Whether a saved file ends with a line break after its final end-of-file\n\
             # marker. The standard requires every line to be terminated AND says the\n\
             # last line contains only the marker; both readings are legitimate.\n\
             #   lf   = end with a line break (default). Always safe.\n\
             #   none = end at the last character of the marker.\n",
        );
        let _ = writeln!(
            out,
            "trailing_eol = {}",
            trailing_eol_token(self.trailing_eol)
        );

        out
    }

    /// Write the settings to `location`, creating the directory if needed.
    ///
    /// # Errors
    ///
    /// [`SaveError`] — there is no writable home, the directory could not
    /// be created, or the write itself failed. Unlike loading, saving
    /// *does* fail loudly: the operator asked for something to be
    /// remembered and is owed the truth if it was not.
    pub fn save(&self, location: &StoreLocation) -> Result<(), SaveError> {
        let Some(path) = location.path.as_ref() else {
            return Err(SaveError::NoWritableLocation);
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| SaveError::Io {
                path: parent.to_path_buf(),
                reason: error.to_string(),
            })?;
        }
        std::fs::write(path, self.write_to_string()).map_err(|error| SaveError::Io {
            path: path.clone(),
            reason: error.to_string(),
        })
    }
}

/// Why settings could not be written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SaveError {
    /// Neither the portable location nor the platform fallback is usable.
    #[error(
        "no writable location for settings: the program folder is not writable and no \
         platform configuration directory could be determined"
    )]
    NoWritableLocation,
    /// The filesystem refused.
    #[error("could not write settings to {path}: {reason}")]
    Io {
        /// What was being written.
        path: PathBuf,
        /// The operating system's reason.
        reason: String,
    },
}

/// Decide where settings live, preferring the portable location.
///
/// Tries `<exe dir>/userdata/` first and falls back to the platform
/// configuration directory only if that cannot be written. Returns
/// [`StoreKind::None`] when neither works, which is still a usable state:
/// defaults load, the session runs, and only [`Settings::save`] fails.
///
/// # Why writability is tested rather than assumed
///
/// `ARCHITECTURE.md` §6 requires pdfcer to run read-only-folder-clean, and
/// the failure this avoids is the one that only shows up in the field: a
/// program that assumes it can write beside itself works perfectly on the
/// developer's machine and fails the first time someone installs it under
/// `Program Files`. The probe is a create-and-remove of a temporary file,
/// which is the only test that answers the actual question — directory
/// permissions on Windows can permit `create_dir_all` and still refuse the
/// write.
#[must_use]
pub fn resolve_store() -> StoreLocation {
    // ★ RESOLVED ONCE PER PROCESS, and that is a correctness property rather
    // than a performance one.
    //
    // The `pdfcer-gui` session's report (2026-08-13) found the write probe's
    // shared-filename race by way of its SHARPEST symptom: two callers in one
    // process DISAGREEING — the layout store resolving `Portable` while the
    // recent list resolved `PlatformFallback`, so two files meant to sit beside
    // each other did not.
    //
    // Fixing the probe makes that disagreement unlikely. Caching makes it
    // IMPOSSIBLE: every caller in a process now gets one answer by
    // construction, whatever the filesystem does underneath. That is the
    // difference between fixing an instance and closing the class, and it was
    // their suggestion — "a stronger property than making the probe reliable".
    //
    // It is also called at least three times per start-up (settings, layout,
    // recent list), each time doing filesystem work, so the saving is real; it
    // is simply not the reason.
    //
    // WHAT THIS DELIBERATELY GIVES UP: a directory that becomes writable
    // MID-RUN is not noticed. Accepted, because the inputs — `current_exe()`
    // and the platform env vars — do not meaningfully change within a process,
    // and because a store that moves under a running application is a worse
    // outcome than one that is stale. `store_in` remains the escape hatch for
    // an explicit directory (tests, and a future `--user-data-dir`), and it
    // does not consult this cache.
    static RESOLVED: std::sync::OnceLock<StoreLocation> = std::sync::OnceLock::new();
    RESOLVED.get_or_init(resolve_store_uncached).clone()
}

/// The uncached resolution [`resolve_store`] memoises. See its doc comment for
/// why the memoisation is a correctness property.
fn resolve_store_uncached() -> StoreLocation {
    if let Some(dir) = portable_dir()
        && directory_is_writable(&dir)
    {
        return StoreLocation {
            path: Some(dir.join(SETTINGS_FILE)),
            kind: StoreKind::Portable,
        };
    }
    if let Some(dir) = platform_dir()
        && directory_is_writable(&dir)
    {
        return StoreLocation {
            path: Some(dir.join(SETTINGS_FILE)),
            kind: StoreKind::PlatformFallback,
        };
    }
    StoreLocation {
        path: None,
        kind: StoreKind::None,
    }
}

/// A store rooted at an explicit directory — for tests and for a future
/// `--user-data-dir` override.
#[must_use]
pub fn store_in(dir: &Path) -> StoreLocation {
    StoreLocation {
        path: Some(dir.join(SETTINGS_FILE)),
        kind: StoreKind::Portable,
    }
}

/// `<directory of the running executable>/userdata`.
fn portable_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join(USER_STATE_DIR))
}

/// The platform configuration directory, without a `dirs`-style
/// dependency.
///
/// Three environment variables cover the three supported platforms, and
/// each is the one the platform's own convention names. Doing this by hand
/// rather than adding a crate keeps a dependency out of `pdfcer-core` for
/// roughly fifteen lines of logic — and this path is the *fallback*, so it
/// is exercised rarely and must stay simple enough to reason about
/// without running it.
fn platform_dir() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
    }?;
    Some(base.join("pdfcer"))
}

/// Whether `dir` can actually be written to, creating it if necessary.
fn directory_is_writable(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    // ★ THE PROBE NAME MUST BE UNIQUE PER CALL.
    //
    // Until 2026-08-13 this was the fixed name `.pdfcer-write-probe`, shared by
    // every caller in every thread and every process. One caller's
    // `remove_file` races another's `write` on the same path, the `write`
    // fails, and this function answers `false` FOR A DIRECTORY THAT IS PLAINLY
    // WRITABLE.
    //
    // Measured by the `pdfcer-gui` session, which reported it: 8 threads x 2,000
    // iterations against one writable temp directory produced **1,223 false
    // negatives in 16,000 calls, ~7.6 %**. Not a rare interleaving.
    //
    // WHY A FALSE `false` IS WORSE THAN AN ERROR. `resolve_store` uses this to
    // choose between the PORTABLE directory beside the executable and the
    // PLATFORM FALLBACK. A spurious `false` does not surface as a failure — it
    // produces a different, valid-looking answer, silently relocating
    // settings, layout and the recent list to the platform config directory.
    // `package-portable.py`'s `BUILD-INFO.txt` tells the operator to "replace
    // the binaries but KEEP `userdata/`", which is only true if the portable
    // directory was the one chosen.
    //
    // The sharper failure, and how they found it: TWO CALLERS IN ONE PROCESS
    // DISAGREEING — the layout store resolving `Portable` while the recent list
    // resolves `PlatformFallback`, so two files meant to sit beside each other
    // do not.
    //
    // Process id AND a counter, because neither alone suffices: two processes
    // share a counter's starting value, and one process's threads share its
    // pid. Needs no dependency.
    //
    // A leftover probe from a killed process is now named distinctly and is
    // therefore harmless, where a stale `.pdfcer-write-probe` was a name the
    // next run would collide with.
    static PROBE_SEQ: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
    let probe = dir.join(format!(
        ".pdfcer-write-probe.{}.{}",
        std::process::id(),
        PROBE_SEQ.fetch_add(1, core::sync::atomic::Ordering::Relaxed)
    ));
    if std::fs::write(&probe, b"").is_err() {
        return false;
    }
    // A failure to clean up does not make the directory unwritable — the
    // write already proved the point.
    let _ = std::fs::remove_file(&probe);
    true
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod probe_race_tests {
    use super::directory_is_writable;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// ★ Concurrent probes of ONE writable directory must all answer true.
    ///
    /// Reported by the `pdfcer-gui` session with a measured reproduction: the
    /// probe used a fixed filename, so one caller's `remove_file` raced
    /// another's `write` and the function answered `false` for a writable
    /// directory — **1,223 of 16,000 calls, ~7.6 %**.
    ///
    /// Thread and iteration counts mirror that reproduction closely enough to
    /// hit the same interleaving; at their rate 8 x 1,000 would expect ~600
    /// failures before the fix. **Zero** is asserted rather than "few", because
    /// unique names make the collision impossible by construction rather than
    /// merely unlikely.
    #[test]
    fn concurrent_probes_never_call_a_writable_directory_unwritable() {
        let dir = std::env::temp_dir().join(format!("pdfcer-probe-race-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");

        let bad = Arc::new(AtomicUsize::new(0));
        std::thread::scope(|s| {
            for _ in 0..8 {
                let dir = dir.clone();
                let bad = Arc::clone(&bad);
                s.spawn(move || {
                    for _ in 0..1000 {
                        if !directory_is_writable(&dir) {
                            bad.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                });
            }
        });

        let bad = bad.load(Ordering::Relaxed);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            bad, 0,
            "{bad} of 8000 probes called a WRITABLE directory unwritable. In production this does not error -- it silently relocates settings, layout and the recent list out of the portable userdata/ directory."
        );
    }

    /// The probe cleans up after itself.
    ///
    /// Unique names make a leftover harmless, but 8,000 of them would be a
    /// different defect. Asserted separately so "unique" cannot be satisfied by
    /// "never deleted".
    #[test]
    fn probing_leaves_no_files_behind() {
        let dir = std::env::temp_dir().join(format!("pdfcer-probe-litter-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        for _ in 0..50 {
            assert!(directory_is_writable(&dir));
        }
        let leftovers: Vec<String> = std::fs::read_dir(&dir)
            .expect("readable")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains("write-probe"))
            .collect();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            leftovers.is_empty(),
            "probe files left behind: {leftovers:?}"
        );
    }

    /// ★ Every caller in a process gets the SAME store, by construction.
    ///
    /// The reported defect's sharpest symptom was two callers in one process
    /// disagreeing — the layout store resolving `Portable` while the recent
    /// list resolved `PlatformFallback`. The probe fix makes that unlikely;
    /// the `OnceLock` makes it impossible, which is the property actually
    /// wanted. Hammered from threads because that is where the disagreement
    /// arose.
    #[test]
    fn every_caller_in_a_process_resolves_the_same_store() {
        let first = super::resolve_store();
        let all_same = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        std::thread::scope(|s| {
            for _ in 0..8 {
                let first = first.clone();
                let all_same = std::sync::Arc::clone(&all_same);
                s.spawn(move || {
                    for _ in 0..200 {
                        let got = super::resolve_store();
                        if got.kind != first.kind || got.path != first.path {
                            all_same.store(false, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                });
            }
        });
        assert!(
            all_same.load(std::sync::atomic::Ordering::Relaxed),
            "two callers in one process resolved DIFFERENT stores — this is the defect that put the layout file and the recent-file list in different directories"
        );
    }

    /// An unwritable directory is still reported unwritable.
    ///
    /// The fix must not turn the function into one that always says yes --
    /// which is the cheapest way to make the race test pass and would be
    /// strictly worse than the bug.
    #[test]
    fn a_path_that_cannot_be_a_directory_is_still_refused() {
        let file = std::env::temp_dir().join(format!("pdfcer-probe-file-{}", std::process::id()));
        std::fs::write(&file, b"x").expect("write");
        // A FILE, not a directory: create_dir_all must fail on it.
        let answer = directory_is_writable(&file);
        let _ = std::fs::remove_file(&file);
        assert!(!answer, "a regular file is not a writable directory");
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::float_cmp
)]
mod tests {
    use super::*;

    #[test]
    fn no_default_is_restated_in_this_module() {
        // A regression guard against someone "simplifying" the manual
        // Default impl back to a hard-coded number. Every default must
        // come from the type that owns the behaviour, or "the default"
        // starts meaning two things depending on whether a settings file
        // happens to exist.
        let engine = crate::text_extract::ExtractOptions::default();
        assert!((Settings::default().word_gap_ratio - engine.word_gap_ratio).abs() < f32::EPSILON);
        assert_eq!(Settings::default().separations, SeparationPolicy::default());
        assert_eq!(Settings::default().cmyk_intent, CmykIntent::default());

        // The ambiguity-register settings whose value is carried by an
        // option struct elsewhere in the crate: the same rule applies, and
        // the assertion is what stops the two drifting.
        assert_eq!(
            Settings::default().unmappable_code,
            engine.unmappable_code,
            "the extraction engine owns the sentinel default"
        );
        assert_eq!(
            Settings::default().actual_text,
            engine.actual_text,
            "the extraction engine owns the /ActualText precedence default"
        );
        let writer = crate::writer::SaveOptions::default();
        assert_eq!(Settings::default().xref_entry_eol, writer.xref_entry_eol);
        assert_eq!(Settings::default().trailing_eol, writer.trailing_eol);

        // And the ones whose only home is the enum itself.
        assert_eq!(Settings::default().mask_resample, MaskResample::default());
        assert_eq!(Settings::default().image_minify, MinifyFilter::default());
        assert_eq!(
            Settings::default().cmyk_jpeg_polarity,
            CmykJpegPolarity::default()
        );
        assert_eq!(
            Settings::default().missing_as,
            MissingAppearanceState::default()
        );
    }

    #[test]
    fn every_shipped_default_is_the_behaviour_that_shipped_before_the_setting() {
        // R169 says a shipped default is "the best guess of what is usually
        // followed", and for every entry the ambiguity register triaged out
        // of already-shipped code that guess is, by construction, WHAT
        // PDFCER ALREADY DID. This test is the guard against a later session
        // flipping one of them on its own authority: adding the knob must
        // not change a single observable behaviour, so each default is
        // pinned to the variant the pre-settings code hard-coded.
        let d = Settings::default();
        assert_eq!(d.mask_resample, MaskResample::Nearest, "mask.rs was NN");
        // ★ DELIBERATE EXCEPTION #2 — `image_minify`, 2026-08-25, and it is
        // an OPERATOR OBSERVATION, not a later session flipping a default on
        // its own authority.
        //
        // This line asserted `PointSample` until today, with the reason
        // "interpret.rs point-sampled in both directions" — true, and the
        // right pin while the default's own evidence tier was (d), a guess.
        //
        // `MinifyFilter`'s doc comment named the exact condition that would
        // move it: "a viewer-behaviour check filed to `C:\personal_rag\pdf\`
        // would raise this to tier (c) and, if it confirms, flip the
        // default." The operator ran that check against Acrobat Reader on
        // his own drawings and reported pdfcer "a little worse than it was,
        // whereas before it was on par" — and named the mechanism from the
        // symptom, unprompted, which is the strongest form the observation
        // could take.
        //
        // ★★ WHY THIS DOES NOT WEAKEN THE GUARANTEE THIS TEST EXISTS FOR.
        // The guarantee is that ADDING A KNOB changes nothing — that a
        // session cannot smuggle a behaviour change in behind a setting.
        // It is NOT that a default may never change afterwards on evidence:
        // that would make every default permanent the moment it shipped,
        // including the ones this file openly labels guesses, and would turn
        // an honesty mechanism into a ratchet. The exception is admissible
        // precisely because it is dated, attributed, and its evidence is
        // written down where a reader will meet it.
        //
        // What a future session may NOT do is add a third exception on its
        // own reasoning. Two rulings do not make a habit; both of these
        // carry the operator's own words.
        assert_eq!(
            d.image_minify,
            MinifyFilter::Smooth,
            "operator viewer check vs Acrobat, 2026-08-25 — see MinifyFilter's docs"
        );
        assert_eq!(
            d.cmyk_jpeg_polarity,
            CmykJpegPolarity::NeverInvert,
            "R29: pdfcer never inverted"
        );
        assert_eq!(
            d.unmappable_code,
            UnmappableCode::ReplacementChar,
            "the ladder's rung 4 emitted U+FFFD"
        );
        assert_eq!(
            d.actual_text,
            ActualTextPrecedence::Always,
            "/ActualText always won"
        );
        assert_eq!(
            d.missing_as,
            MissingAppearanceState::PaintNothing,
            "a multi-entry /N with no /AS painted nothing"
        );
        // THE ONE DELIBERATE EXCEPTION, and it is an operator ruling, not
        // a later session flipping a default on its own authority — which
        // is the thing this test exists to prevent.
        //
        // Ken, 2026-08-08: "change the shipped default so that we match
        // the file's existing 2-byte EOL." The register had recommended
        // exactly that and said the shipped fixed `SP LF` was "arguably
        // wrong on pdfcer's own invariant": a full rewrite of a `CR LF`
        // file changes two bytes in every entry of a document nobody
        // edited, which is the diff rule 3 exists to prevent.
        //
        // The guarantee this test protects is NOT broken by that, and the
        // distinction is worth being precise about. `MatchSource` on an
        // `SP LF` source resolves to `SP LF` — so for every file pdfcer
        // previously round-tripped byte-identically it still does. What
        // changed is the answer for files pdfcer was previously getting
        // WRONG. Pinned from both sides: the resolution below, and
        // `tests/xref_eol.rs::a_full_rewrite_keeps_the_files_own_entry_eol`.
        assert_eq!(
            d.xref_entry_eol,
            XrefEntryEol::MatchSource,
            "operator ruling 2026-08-08: the default matches the source file"
        );
        assert_eq!(
            XrefEntryEol::MatchSource.resolve(b""),
            XrefEntryEol::SpaceLf,
            "with nothing to match, the answer is still what xref_out.rs always emitted"
        );
        assert_eq!(
            d.trailing_eol,
            TrailingEol::Lf,
            "xref_out.rs emitted an LF after %%EOF"
        );
    }

    #[test]
    fn an_empty_file_yields_defaults_quietly() {
        let mut notes = Vec::new();
        let settings = Settings::parse("", &mut notes);
        assert_eq!(settings, Settings::default());
        assert!(notes.is_empty());
    }

    #[test]
    fn comments_and_blank_lines_are_not_content() {
        let mut notes = Vec::new();
        let settings = Settings::parse("# a comment\n\n   \n\t# indented\n", &mut notes);
        assert_eq!(settings, Settings::default());
        assert!(notes.is_empty(), "no note for a file of pure commentary");
    }

    #[test]
    fn every_setting_round_trips_through_the_file() {
        // The test that keeps `write_to_string` and `apply` from drifting:
        // a setting that can be written but not read back is a setting
        // that silently resets on the next launch.
        //
        // Every field is set to a value that is NOT its default, so a key
        // that `write_to_string` forgot cannot pass by accidentally
        // matching the default on the way back in.
        let written = Settings {
            separations: SeparationPolicy::Discard,
            // ★ Was `Calibrated`, WHICH IS THE DEFAULT -- so this one field
            // broke the discipline the comment above states, and if
            // `write_to_string` had forgotten `cmyk_intent` entirely this
            // test would have passed. Found 2026-08-30 while adding
            // `style_policy` below. `NeutralBlack` is the non-default.
            cmyk_intent: CmykIntent::NeutralBlack,
            // NOT the default (`Auto`), same reason.
            style_policy: StylePolicy::Refuse,
            // NOT the default (`OutputIntentIfSubtractive`) -- see the note
            // above about a value that matches the default proving nothing.
            page_blend_space_source: PageBlendSpaceSource::DeviceNative,
            // NOT the default (`DeviceCmykOnly` since `Pass 244.0`), same reason.
            overprint_zero_tint_scope: OverprintZeroTintScope::GreyAsKOnly,
            // Non-default, per the discipline the comment above states.
            spot_colorant_device_model: SpotColorantDeviceModel::AlternateSpaceSubstitution,
            // NOT the default (`PerRecord`), same reason.
            mesh_patch_padding: MeshPatchPadding::None,
            word_gap_ratio: 0.35,
            // Deliberately NOT the default (0.5): this test exists to catch a
            // field `write_to_string` forgot, and a value equal to the default
            // would pass by accident on the way back in.
            parallel_epsilon_degrees: 1.25,
            mask_resample: MaskResample::BoxAverage,
            image_minify: MinifyFilter::Smooth,
            cmyk_jpeg_polarity: CmykJpegPolarity::InvertOnApp14,
            unmappable_code: UnmappableCode::Omit,
            actual_text: ActualTextPrecedence::Glyphs,
            missing_as: MissingAppearanceState::FirstEntry,
            quad_point_order: QuadPointOrder::Counterclockwise,
            xref_entry_eol: XrefEntryEol::CrLf,
            trailing_eol: TrailingEol::None,
            // NOT the default (`None`), and deliberately a value that is a
            // whole number of GiB so the friendly writer's suffix branch is
            // the one exercised — a round trip that only ever went through
            // the bare-integer branch would not prove the suffix parses.
            max_cmyk_buffer_bytes: Some(2 * 1024 * 1024 * 1024),
            // A token core does NOT know, on purpose: this pins that the
            // round trip preserves whatever the shell wrote rather than
            // normalising it to something core recognises — which is the
            // whole point of storing it opaquely.
            theme: "dark".to_owned(),
        };
        assert_ne!(
            written,
            Settings::default(),
            "the round-trip fixture must not be the default settings"
        );
        let mut notes = Vec::new();
        let read = Settings::parse(&written.write_to_string(), &mut notes);
        assert_eq!(read, written);
        assert!(notes.is_empty(), "pdfcer's own output must parse cleanly");
    }

    #[test]
    fn the_default_settings_round_trip_too() {
        let mut notes = Vec::new();
        let read = Settings::parse(&Settings::default().write_to_string(), &mut notes);
        assert_eq!(read, Settings::default());
        assert!(notes.is_empty());
    }

    #[test]
    fn one_bad_value_does_not_discard_the_good_ones() {
        // The whole reason this is not a serde derive.
        let mut notes = Vec::new();
        let settings = Settings::parse(
            "separations = discard\ncmyk_intent = purple\nword_gap_ratio = 0.4\n",
            &mut notes,
        );
        assert_eq!(settings.separations, SeparationPolicy::Discard);
        assert_eq!(
            settings.cmyk_intent,
            CmykIntent::default(),
            "an unreadable value falls back to the default, whatever it currently is"
        );
        assert!((settings.word_gap_ratio - 0.4).abs() < f32::EPSILON);
        assert_eq!(
            notes,
            vec![SettingNote::BadValue {
                key: "cmyk_intent".to_owned(),
                value: "purple".to_owned(),
                line: 2,
                using: cmyk_token(CmykIntent::default()).to_owned(),
            }]
        );
    }

    #[test]
    fn an_unknown_key_is_reported_at_its_line_and_nothing_else_breaks() {
        let mut notes = Vec::new();
        let settings = Settings::parse("ribbon_layout = wide\nseparations = refuse\n", &mut notes);
        assert_eq!(settings.separations, SeparationPolicy::Refuse);
        assert_eq!(
            notes,
            vec![SettingNote::UnknownKey {
                key: "ribbon_layout".to_owned(),
                line: 1,
            }]
        );
    }

    #[test]
    fn a_line_with_no_equals_is_malformed_and_skipped() {
        let mut notes = Vec::new();
        let settings = Settings::parse("this is not a setting\nseparations = refuse\n", &mut notes);
        assert_eq!(settings.separations, SeparationPolicy::Refuse);
        assert_eq!(notes, vec![SettingNote::Malformed { line: 1 }]);
    }

    #[test]
    fn an_out_of_range_number_is_clamped_and_said_so() {
        let mut notes = Vec::new();
        let settings = Settings::parse("word_gap_ratio = 99\n", &mut notes);
        assert!((settings.word_gap_ratio - MAX_WORD_GAP_RATIO).abs() < f32::EPSILON);
        assert_eq!(
            notes,
            vec![SettingNote::Clamped {
                key: "word_gap_ratio".to_owned(),
                value: "99".to_owned(),
                line: 1,
                using: MAX_WORD_GAP_RATIO.to_string(),
            }]
        );
    }

    #[test]
    fn a_byte_size_is_accepted_in_every_form_an_operator_would_write_it() {
        // 256 MiB, written six ways. All six must mean the same number —
        // the point of accepting more than one spelling is that the
        // operator never has to guess which one the file wants, and that
        // guarantee is only real if it is tested.
        const EXPECT: usize = 256 * 1024 * 1024;
        for text in [
            "268435456",
            "256mib",
            "256 MiB",
            "256mb",
            "256MB",
            "0.25gb",
            "268435456b",
        ] {
            let mut notes = Vec::new();
            let settings =
                Settings::parse(&format!("max_cmyk_buffer_bytes = {text}\n"), &mut notes);
            assert_eq!(
                settings.max_cmyk_buffer_bytes,
                Some(EXPECT),
                "{text:?} should mean 256 MiB"
            );
            assert!(notes.is_empty(), "{text:?} should parse cleanly");
        }
    }

    #[test]
    fn an_unreadable_byte_size_falls_back_rather_than_becoming_zero() {
        // The failure worth naming: a size that silently became `Some(0)`
        // would turn "I typed something wrong" into "never composite in
        // ink", which looks like a rendering regression rather than a typo.
        // Zero itself is legal and IS that instruction, said deliberately.
        for text in ["-1", "lots", "", "8gx", "NaN", "1e400gib"] {
            let mut notes = Vec::new();
            let settings =
                Settings::parse(&format!("max_cmyk_buffer_bytes = {text}\n"), &mut notes);
            assert_eq!(
                settings.max_cmyk_buffer_bytes, None,
                "{text:?} must fall back to the default"
            );
            assert_eq!(notes.len(), 1, "{text:?} must be reported, not swallowed");
        }
        let mut notes = Vec::new();
        let settings = Settings::parse("max_cmyk_buffer_bytes = 0\n", &mut notes);
        assert_eq!(settings.max_cmyk_buffer_bytes, Some(0));
        assert!(notes.is_empty());
    }

    #[test]
    fn every_byte_size_round_trips_through_the_friendly_writer() {
        // `format_byte_size` is allowed to be friendly and is NOT allowed
        // to be lossy: a writer that rounded 1.5 GiB to "1gib" would edit
        // the operator's setting every time pdfcer saved the file, which is
        // indistinguishable from pdfcer ignoring it.
        for bytes in [
            None,
            Some(0),
            Some(1),
            Some(1023),
            Some(256 * 1024 * 1024),
            Some(3 * 1024 * 1024 * 1024 / 2),
            Some(5 * 1024 * 1024 * 1024),
            Some(usize::from(u16::MAX) * 7919),
        ] {
            let text = format_byte_size(bytes);
            assert_eq!(parse_byte_size(&text), Ok(bytes), "wrote {text:?}");
        }
    }

    #[test]
    fn a_non_finite_number_is_a_bad_value_not_a_clamp() {
        // `NaN.clamp(..)` and `inf.clamp(..)` do not do what a reader
        // expects, so they are rejected before the clamp rather than
        // silently becoming a bound.
        for text in ["word_gap_ratio = NaN\n", "word_gap_ratio = inf\n"] {
            let mut notes = Vec::new();
            let settings = Settings::parse(text, &mut notes);
            assert_eq!(settings.word_gap_ratio, Settings::default().word_gap_ratio);
            assert!(matches!(notes.as_slice(), [SettingNote::BadValue { .. }]));
        }
    }

    #[test]
    fn the_last_duplicate_wins_and_the_duplication_is_reported() {
        let mut notes = Vec::new();
        let settings = Settings::parse("separations = discard\nseparations = refuse\n", &mut notes);
        assert_eq!(settings.separations, SeparationPolicy::Refuse, "last wins");
        assert_eq!(
            notes,
            vec![SettingNote::Duplicate {
                key: "separations".to_owned(),
                line: 2,
            }]
        );
    }

    #[test]
    fn whitespace_around_keys_and_values_is_not_significant() {
        let mut notes = Vec::new();
        let settings = Settings::parse("   separations   =   refuse   \n", &mut notes);
        assert_eq!(settings.separations, SeparationPolicy::Refuse);
        assert!(notes.is_empty());
    }

    #[test]
    fn a_missing_file_is_silent_but_a_present_one_is_flagged_as_existing() {
        let dir = std::env::temp_dir().join(format!("pdfcer-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let location = store_in(&dir);

        let (settings, report) = Settings::load(location.clone());
        assert_eq!(settings, Settings::default());
        assert!(!report.existed, "a first run has no file");
        assert!(report.is_quiet(), "and a first run is not a fault");

        let written = Settings {
            cmyk_intent: CmykIntent::Calibrated,
            ..Settings::default()
        };
        written.save(&location).expect("save must succeed");
        let (reloaded, report) = Settings::load(location);
        assert_eq!(reloaded, written);
        assert!(report.existed);
        assert!(report.is_quiet());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saving_without_a_location_is_a_named_refusal() {
        let nowhere = StoreLocation {
            path: None,
            kind: StoreKind::None,
        };
        assert_eq!(
            Settings::default().save(&nowhere),
            Err(SaveError::NoWritableLocation)
        );
    }

    #[test]
    fn every_line_of_the_written_file_is_a_comment_a_setting_or_a_blank() {
        // ★ WHY THIS EXISTS, AND IT IS NOT HYPOTHETICAL.
        //
        // The file's comment block is one enormous Rust string literal held
        // together by `\n\` line continuations. Lose one backslash and the
        // literal STILL COMPILES, still round-trips, still passes every
        // other test here — and emits a stray blank line plus thirteen
        // spaces of source indentation into a file pdfcer writes onto the
        // operator's own disk. That happened on 2026-08-26, inside the very
        // commit whose purpose was correcting this paragraph, and it was
        // caught by a reading agent rather than by anything mechanical:
        // `check-string-gaps.sh` looks for a run of spaces INSIDE a
        // sentence, and this defect puts the run at the START of a line,
        // where that gate cannot see it.
        //
        // So the assertion is on the OUTPUT, not on the source. Every line
        // pdfcer writes must be a comment, a `key = value`, or empty —
        // which is exactly what `parse` demands of an operator, and there
        // is no reason pdfcer's own output should be held to a looser
        // standard than the file it will read back.
        let text = Settings::default().write_to_string();
        for (index, line) in text.lines().enumerate() {
            let n = index + 1;
            assert!(
                line.is_empty() || line.starts_with('#') || line.contains(" = "),
                "line {n} of the written file is neither comment, setting nor blank: {line:?}"
            );
            assert_eq!(
                line.trim_end(),
                line,
                "line {n} has trailing whitespace: {line:?}"
            );
            assert_eq!(
                line.trim_start(),
                line,
                "line {n} is INDENTED, which means a `\\n\\` continuation lost \
                 its backslash and leaked the source's own indentation: {line:?}"
            );
        }
        // A blank line separates settings; two in a row means a continuation
        // emitted an extra newline, which is the other half of the same
        // defect and is invisible to the per-line checks above.
        assert!(
            !text.contains("\n\n\n"),
            "the written file has a doubled blank line — a `\\n\\` continuation \
             emitted a raw newline as well as its escape"
        );
    }

    #[test]
    fn the_written_file_names_every_legal_value_of_every_key() {
        // The file is meant to be hand-edited, so a key whose alternatives
        // are undiscoverable from the file itself is a key the operator
        // can only change by reading source.
        let text = Settings::default().write_to_string();
        for token in [
            "repair",
            "discard",
            "refuse",
            "calibrated",
            "neutral_black",
            "nearest",
            "box_average",
            "bilinear",
            "point_sample",
            "smooth",
            "never_invert",
            "invert_on_app14",
            "replacement_char",
            "question_mark",
            "omit",
            "always",
            "tagged_only",
            "glyphs",
            "paint_nothing",
            "first_entry",
            "off_else_nothing",
            "space_lf",
            "space_cr",
            "cr_lf",
            "lf",
            "none",
        ] {
            assert!(
                text.contains(token),
                "{token} is not documented in the file"
            );
        }
        assert!(
            text.contains("KEEP THIS FOLDER"),
            "R15's update instruction must be in the file the update would destroy"
        );
    }
}
