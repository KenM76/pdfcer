//! Natively render what a recognised `AF*_Format` helper would DISPLAY —
//! decision 009 posture B, display side.
//!
//! # The invariant this module exists to protect
//!
//! **A format helper never changes `/V`.** It chooses the string an operator
//! sees; the stored value stays exactly as it was. Nothing here returns
//! anything that a caller could mistake for a value to store: the return type
//! is [`Formatted`], which carries display attributes ([`Formatted::red`])
//! that a `/V` could not possibly hold, so a call site that tried to store a
//! formatted result would have to visibly discard them first.
//!
//! Getting this wrong is data loss, not a cosmetic bug. `"$1,234.00"` does
//! not parse back as `1234`, so a formatted string baked into `/V` destroys
//! the value and every calculation that reads it.
//!
//! ## ★ The standard PERMITS what this invariant forbids
//!
//! Worth stating plainly, because the obvious assumption is the opposite.
//! §12.6.3's Table 196 describes the `F` trigger as firing "before the field
//! is formatted to display its value" and then adds: "This action **may
//! modify the field's value** before formatting." NOTE 2 goes further and
//! uses this exact case as its worked example — even though `F` triggers
//! formatting, "it is possible for an action triggered by this event to
//! perform a calculation or make any other modification to the document".
//! There is no `shall not` anywhere in the clause.
//!
//! So this is a **pdfcer invariant that deliberately declines spec-granted
//! latitude**, not a rule the spec supplies. The reasons are pdfcer's own:
//! a value silently rewritten by a *display* helper is unauditable, it
//! breaks the round-trip discipline, and it is unrecoverable in the way
//! described above. Acrobat's own model reads "the field's value" there as
//! the display string rather than `/V` — but that reading is Acrobat's, and
//! ISO 32000-1 defines no event object to anchor it to.
//!
//! # Sourcing and its limits
//!
//! ISO 32000-1 specifies none of this — see [`super::calc`] for why. The
//! tables below come from `pdfcer-acrobat-librarian`'s
//! `Acrobat_Features/forms__calculation_validation_javascript.md`, which
//! tags every fact by evidence tier. The tiers are carried into the doc
//! comments here rather than flattened, because they differ:
//!
//! - The `AFDate_Format` and `AFTime_Format` index tables are **dual-sourced
//!   and agree index-for-index** — the strongest evidence in the set.
//! - The `sepStyle` and `negStyle` tables are corroborated across
//!   independent sources.
//! - `AFPercent_Format` multiplying by 100, and the empty-value behaviours,
//!   are sourced from Mozilla `pdf.js`'s independent MPL-2.0
//!   reimplementation — **behaviour only; no code was copied and pdfcer links
//!   nothing** (`LEGAL.md` §6.1). Real Acrobat is not separately confirmed
//!   on these, and each is marked where it is used.
//! - `sepStyle` **4** and `currStyle`'s meaning are **unsourced**. Neither is
//!   guessed at: see [`SeparatorStyle::from_code`] and
//!   [`FormatOutcome::UnknownStyle`].
//!
//! # Declining is a first-class result
//!
//! [`FormatOutcome`] has a decline arm, and it is used. A helper pdfcer
//! recognises but cannot faithfully render — an unsourced style code, a
//! value that is not a number — returns the raw stored value with the reason
//! attached, so a shell shows the truth and says it is unformatted. Rendering
//! *something* would be the sneaky half of project rule 4: the operator would
//! see a formatted string and reasonably conclude pdfcer had reproduced the
//! document's intent.

use super::FormatHelper;
use super::calc::{CommaPolicy, parse_number};
use super::datetime;

/// Grouping and decimal separator convention (`sepStyle`).
///
/// Four documented modes. The pair of characters is the whole content of the
/// setting; every mode is one of the two separators crossed with whether
/// grouping happens at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeparatorStyle {
    /// The thousands separator, or `None` when digits are not grouped.
    pub group: Option<char>,
    /// The decimal separator.
    pub decimal: char,
}

impl SeparatorStyle {
    /// Map a `sepStyle` code to its convention.
    ///
    /// # Code 4 was unsourced and is now MEASURED
    ///
    /// No source of any tier described mode 4 — one reimplementation clamps
    /// the parameter to `[0, 4]`, which implied a fifth mode existed without
    /// saying what it rendered, and pdfcer declined it rather than guess.
    ///
    /// A probe form was opened in the installed Acrobat on 2026-08-11 with
    /// `AFNumber_Format(2, 4, 0, 0, "", true)` over a stored `1234.56`, beside
    /// a `sepStyle 0` control. Acrobat displayed **`1'234.56`** against the
    /// control's `1,234.56`: an **apostrophe** thousands separator with a
    /// period decimal — the Swiss convention. Recorded as measured
    /// first-party behaviour, which is a stronger tier than anything else in
    /// this table.
    ///
    /// Returns `None` for **5 and above**, still undescribed by anything.
    #[must_use]
    pub const fn from_code(code: i64) -> Option<Self> {
        match code {
            0 => Some(Self {
                group: Some(','),
                decimal: '.',
            }),
            1 => Some(Self {
                group: None,
                decimal: '.',
            }),
            2 => Some(Self {
                group: Some('.'),
                decimal: ',',
            }),
            3 => Some(Self {
                group: None,
                decimal: ',',
            }),
            // Measured, not guessed — see this function's own doc comment.
            4 => Some(Self {
                group: Some('\''),
                decimal: '.',
            }),
            _ => None,
        }
    }
}

/// How a negative value is presented (`negStyle`).
///
/// The four codes are not four unrelated cases: they are two independent
/// choices — bracket or not, red or not — enumerated. Modelling them as the
/// two booleans they are keeps the rendering code from growing a four-arm
/// match that could disagree with itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NegativeStyle {
    /// Wrap the magnitude in parentheses instead of showing a minus sign.
    pub parenthesise: bool,
    /// Render in red.
    pub red: bool,
}

impl NegativeStyle {
    /// Map a `negStyle` code, or `None` if it is outside the documented 0–3.
    #[must_use]
    pub const fn from_code(code: i64) -> Option<Self> {
        match code {
            0 => Some(Self {
                parenthesise: false,
                red: false,
            }),
            1 => Some(Self {
                parenthesise: false,
                red: true,
            }),
            2 => Some(Self {
                parenthesise: true,
                red: false,
            }),
            3 => Some(Self {
                parenthesise: true,
                red: true,
            }),
            _ => None,
        }
    }
}

/// A rendered display string, plus the attributes that are not part of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Formatted {
    /// The string to display.
    pub text: String,
    /// Whether it should be drawn in red (`negStyle` 1 or 3, and only for a
    /// negative value).
    ///
    /// Carried separately because it is not expressible in the text, and
    /// because its presence on this type is what makes [`Formatted`]
    /// structurally unmistakable for a value to store.
    pub red: bool,
}

impl Formatted {
    /// A plain, black display string.
    fn plain(text: String) -> Self {
        Self { text, red: false }
    }
}

/// The result of asking pdfcer to render a formatted display string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatOutcome {
    /// pdfcer reproduced the helper's output.
    Rendered(Formatted),
    /// The helper is recognised, but a parameter's meaning is unsourced, so
    /// pdfcer will not guess at the output.
    ///
    /// Carries the parameter name and the offending code, so a disclosure can
    /// say exactly what stopped it rather than "could not format".
    UnknownStyle {
        /// Which parameter (`sepStyle`, `negStyle`, `psf`).
        parameter: &'static str,
        /// The value the script passed.
        code: i64,
    },
    /// The stored value is not a number, so a numeric helper has nothing to
    /// format.
    ///
    /// Distinct from [`FormatOutcome::UnknownStyle`]: pdfcer understood the
    /// helper perfectly and the *document* has nothing formattable.
    NotNumeric,
    /// The stored value could not be read as a date **unambiguously**.
    ///
    /// Not a parser shortfall to apologise for. `03/04/2026` genuinely does
    /// not determine a date, and the alternative to declining is rendering a
    /// confident wrong one — which on a form is indistinguishable from a
    /// right one. See [`super::datetime::parse`] for what is accepted.
    NotADate,
    /// The helper is one pdfcer recognises but does not render in this cut.
    Unsupported {
        /// A stable token naming the helper.
        helper: &'static str,
    },
}

impl FormatOutcome {
    /// The rendered display string, if there is one.
    #[must_use]
    pub const fn text(&self) -> Option<&String> {
        match self {
            Self::Rendered(f) => Some(&f.text),
            _ => None,
        }
    }

    /// A short operator-facing reason a value is shown unformatted.
    ///
    /// `None` when there is nothing to explain. Every other arm explains
    /// itself concretely — "pdfcer could not format this" tells an operator
    /// only that something went wrong, which is the disclosure that reads as
    /// a defect rather than as a boundary.
    #[must_use]
    pub fn decline_reason(&self) -> Option<String> {
        match self {
            Self::Rendered(_) => None,
            Self::UnknownStyle { parameter, code } => Some(format!(
                "the script passes {parameter}={code}, a setting no available source \
                 documents; pdfcer shows the stored value rather than guess at how \
                 Acrobat renders it"
            )),
            Self::NotNumeric => {
                Some("the stored value is not a number, so there is nothing to format".to_owned())
            }
            Self::NotADate => Some(
                "the stored value does not name a date unambiguously — a value like \
                 03/04/2026 means different days in different countries, so pdfcer shows \
                 it as stored rather than guess"
                    .to_owned(),
            ),
            Self::Unsupported { helper } => Some(format!(
                "pdfcer recognises {helper} but does not yet reproduce its display"
            )),
        }
    }
}

/// Render what a recognised format helper would display for `stored_value`.
///
/// `stored_value` is the field's raw `/V` text. It is never modified, and
/// nothing this function returns is intended to replace it.
#[must_use]
pub fn render(helper: &FormatHelper, stored_value: &str, policy: CommaPolicy) -> FormatOutcome {
    match helper {
        FormatHelper::Number {
            decimals,
            separator_style,
            negative_style,
            currency,
            prepend_currency,
            // `currStyle` is reserved and inert — described that way by
            // every source, and now MEASURED: a probe opened in the
            // installed Acrobat on 2026-08-11 rendered `currStyle` 1 and 0
            // identically (`1,234.56` both) over the same stored value.
            // pdfcer accepts and ignores it, which is now a match rather than
            // an assumption.
            currency_style: _,
        } => number(
            stored_value,
            *decimals,
            *separator_style,
            *negative_style,
            currency,
            *prepend_currency,
            policy,
        ),
        FormatHelper::Percent {
            decimals,
            separator_style,
        } => percent(stored_value, *decimals, *separator_style, policy),
        FormatHelper::Special { selector } => special(stored_value, *selector),
        FormatHelper::Date { index } => predefined_datetime(
            stored_value,
            datetime::date_format(*index),
            "pdfFormat",
            *index,
        ),
        FormatHelper::Time { index } => predefined_datetime(
            stored_value,
            datetime::time_format(*index),
            "pdfFormat",
            *index,
        ),
        FormatHelper::DateEx { format } => date_through(stored_value, format),
    }
}

/// `AFNumber_Format`.
#[allow(clippy::too_many_arguments)]
fn number(
    stored: &str,
    decimals: i64,
    sep_code: i64,
    neg_code: i64,
    currency: &[u8],
    prepend: bool,
    policy: CommaPolicy,
) -> FormatOutcome {
    let Some(sep) = SeparatorStyle::from_code(sep_code) else {
        return FormatOutcome::UnknownStyle {
            parameter: "sepStyle",
            code: sep_code,
        };
    };
    let Some(neg) = NegativeStyle::from_code(neg_code) else {
        return FormatOutcome::UnknownStyle {
            parameter: "negStyle",
            code: neg_code,
        };
    };
    // An empty or non-numeric value displays as nothing at all — the helper
    // never invents a formatted zero for a box the user left blank. (This
    // resolves a question posture A left open; sourced from the independent
    // reimplementation, not Adobe-primary.) A field showing `0.00` for an
    // untouched box would misreport an unfilled form as filled.
    let Some(value) = parse_number(stored, policy) else {
        return FormatOutcome::Rendered(Formatted::plain(String::new()));
    };
    let currency = String::from_utf8_lossy(currency).into_owned();
    FormatOutcome::Rendered(compose(value, decimals, sep, neg, &currency, prepend))
}

/// `AFPercent_Format`.
///
/// **The stored value is multiplied by 100.** A stored `0.085` displays as
/// `8.5%`. This is the single detail most often got wrong by assumption, and
/// it is sourced rather than assumed — though from the independent
/// reimplementation rather than from Adobe directly.
///
/// The consequence for pdfcer's data model is worth stating plainly: a
/// percent-formatted field stores a **fraction**, and the ×100 belongs at
/// display time only. A recompute that wrote `8.5` into such a field would
/// make it display as `850%`.
fn percent(stored: &str, decimals: i64, sep_code: i64, policy: CommaPolicy) -> FormatOutcome {
    let Some(sep) = SeparatorStyle::from_code(sep_code) else {
        return FormatOutcome::UnknownStyle {
            parameter: "sepStyle",
            code: sep_code,
        };
    };
    // ★ MEASURED, and it falsified what pdfcer had implemented.
    //
    // The single available source — one independent reimplementation — has
    // an empty percent field display a bare `%`, and pdfcer reproduced that,
    // flagging it as possibly the clone's own quirk.
    //
    // A probe form opened in the installed Acrobat on 2026-08-11 with
    // `AFPercent_Format(1, 0)` over an EMPTY value displayed **`0.0%`**, not
    // `%`. So Acrobat coerces an unreadable percent value to **zero** and
    // formats it, where `AFNumber_Format` leaves the field blank — a real
    // asymmetry between the two helpers, and the opposite of what the clone
    // does.
    //
    // Worth noting the shape: the wrong behaviour was reproduced faithfully
    // FROM a source, disclosed as single-tier, and still wrong. Marking a
    // fact as weakly-sourced is not the same as checking it.
    let value = parse_number(stored, policy).unwrap_or(0.0);
    let neg = NegativeStyle {
        parenthesise: false,
        red: false,
    };
    let mut out = compose(value * 100.0, decimals, sep, neg, "", false);
    out.text.push('%');
    FormatOutcome::Rendered(out)
}

/// Compose a formatted number from its already-validated parts.
///
/// Shared by the number and percent paths so grouping, rounding and sign
/// handling cannot drift between them.
fn compose(
    value: f64,
    decimals: i64,
    sep: SeparatorStyle,
    neg: NegativeStyle,
    currency: &str,
    prepend: bool,
) -> Formatted {
    // A negative value's sign is stripped before formatting and reapplied as
    // a wrapper, so parentheses surround the magnitude and the currency
    // symbol rather than landing inside a `-…` string.
    let negative = value < 0.0;
    let magnitude = value.abs();
    let places = decimals.clamp(0, MAX_DECIMALS) as usize;
    let fixed = format!("{magnitude:.places$}");

    let (int_part, frac_part) = fixed
        .split_once('.')
        .map_or((fixed.as_str(), ""), |(i, f)| (i, f));
    let mut text = match sep.group {
        Some(g) => group_digits(int_part, g),
        None => int_part.to_owned(),
    };
    if !frac_part.is_empty() {
        text.push(sep.decimal);
        text.push_str(frac_part);
    }
    if !currency.is_empty() {
        if prepend {
            text.insert_str(0, currency);
        } else {
            text.push_str(currency);
        }
    }
    if negative {
        if neg.parenthesise {
            text.insert(0, '(');
            text.push(')');
        } else {
            text.insert(0, '-');
        }
    }
    Formatted {
        text,
        // Red applies only to a value that is actually negative — the style
        // says how to show a negative, not how to show every value.
        red: negative && neg.red,
    }
}

/// The largest `nDec` pdfcer will honour.
///
/// `f64` carries about 17 significant digits, so beyond this the extra places
/// are formatting noise rather than information. Clamping rather than
/// refusing keeps a script with an absurd `nDec` renderable; the alternative
/// would decline to format a field over a parameter that changes nothing an
/// operator can see.
pub const MAX_DECIMALS: i64 = 15;

/// Insert `separator` every three digits from the right.
fn group_digits(digits: &str, separator: char) -> String {
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    let n = digits.len();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (n - i).is_multiple_of(3) {
            out.push(separator);
        }
        out.push(c);
    }
    out
}

/// `AFDate_Format` / `AFTime_Format` — a predefined format by index.
///
/// An index no source describes declines by name rather than falling back to
/// treating the raw index as a literal format string. One reimplementation
/// does exactly that, which would render a stored `99` as the text `99`;
/// nothing confirms real Acrobat behaves so, and inventing output for an
/// undocumented mode is the guessing this posture exists to refuse.
fn predefined_datetime(
    stored: &str,
    format: Option<&'static str>,
    parameter: &'static str,
    code: i64,
) -> FormatOutcome {
    match format {
        Some(f) => date_through(stored, f.as_bytes()),
        None => FormatOutcome::UnknownStyle { parameter, code },
    }
}

/// Render a stored value through a date/time format string.
///
/// # The parse is the limit, not the grammar
///
/// The token grammar is fully sourced and [`datetime::render`] implements all
/// of it. What nothing describes is how Acrobat reads a stored date string
/// back out of a field, so [`datetime::parse`] accepts only shapes that
/// cannot be read two ways and returns `None` for the rest — most notably for
/// `03/04/2026`, which is 3 April to most of the world and 4 March in the
/// United States.
///
/// A value that will not parse yields [`FormatOutcome::NotADate`] and the
/// stored text is shown unformatted. That is the honest outcome: pdfcer
/// understood the helper perfectly and could not read the document's value,
/// which is a different fact from not supporting the helper, and an operator
/// acting on it would do different things.
fn date_through(stored: &str, format: &[u8]) -> FormatOutcome {
    // An empty field formats to nothing, matching the number path: a blank
    // box must not sprout a date.
    if stored.trim().is_empty() {
        return FormatOutcome::Rendered(Formatted::plain(String::new()));
    }
    match datetime::parse(stored) {
        Some(when) => FormatOutcome::Rendered(Formatted::plain(datetime::render(format, &when))),
        None => FormatOutcome::NotADate,
    }
}

/// `AFSpecial_Format` — the four fixed masks.
///
/// Formats the **digits** of the stored value into a mask. Non-digits in the
/// stored value are dropped before masking, because the stored value of a
/// re-formatted field already contains the previous mask's punctuation and
/// re-masking it would compound the separators.
///
/// A digit count that does not fit the mask returns the stored value
/// unchanged rather than a half-filled mask. What Acrobat does with an
/// over-long value is unsourced, and a partial mask (`(555) 123-45`) looks
/// like data rather than like a formatting failure.
fn special(stored: &str, selector: i64) -> FormatOutcome {
    let digits: String = stored.chars().filter(char::is_ascii_digit).collect();
    let mask = match selector {
        0 => "99999",
        1 => "99999-9999",
        // The phone mask is CONDITIONAL, not fixed: ten or more digits get
        // the area-code form, fewer get the local form. A fixed mask here
        // would mis-render every seven-digit number in the document.
        2 => {
            if digits.len() >= 10 {
                "(999) 999-9999"
            } else {
                "999-9999"
            }
        }
        3 => "999-99-9999",
        code => {
            return FormatOutcome::UnknownStyle {
                parameter: "psf",
                code,
            };
        }
    };
    let wanted = mask.chars().filter(|c| *c == '9').count();
    if digits.len() != wanted {
        return FormatOutcome::Rendered(Formatted::plain(stored.to_owned()));
    }
    let mut it = digits.chars();
    let text = mask
        .chars()
        .map(|c| if c == '9' { it.next().unwrap_or(c) } else { c })
        .collect();
    FormatOutcome::Rendered(Formatted::plain(text))
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

    fn num(
        stored: &str,
        decimals: i64,
        sep: i64,
        neg: i64,
        currency: &str,
        prepend: bool,
    ) -> FormatOutcome {
        render(
            &FormatHelper::Number {
                decimals,
                separator_style: sep,
                negative_style: neg,
                currency_style: 0,
                currency: currency.as_bytes().to_vec(),
                prepend_currency: prepend,
            },
            stored,
            CommaPolicy::default(),
        )
    }

    fn text_of(outcome: &FormatOutcome) -> &str {
        outcome.text().map_or("<declined>", String::as_str)
    }

    /// The five separator conventions render as their table says.
    ///
    /// Modes 0–3 are corroborated across independent sources; mode 4 is
    /// measured first-party behaviour, which is a stronger tier than
    /// anything else in this table.
    #[test]
    fn the_five_separator_styles_render_their_conventions() {
        for (code, want) in [
            (0, "1,234.56"),
            (1, "1234.56"),
            (2, "1.234,56"),
            (3, "1234,56"),
            // ★ Mode 4 was UNSOURCED and is now measured: a probe opened in
            // the installed Acrobat rendered `1'234.56` — an apostrophe
            // thousands separator with a period decimal, the Swiss
            // convention — beside a `sepStyle 0` control showing
            // `1,234.56` in the same document.
            (4, "1'234.56"),
        ] {
            let got = num("1234.56", 2, code, 0, "", false);
            assert_eq!(text_of(&got), want, "sepStyle {code}");
        }
    }

    /// ★ **`currStyle` is inert, and that is now measured rather than
    /// assumed.**
    ///
    /// Every source describes it as reserved. The same probe rendered
    /// `currStyle` 1 and 0 identically over one stored value, so pdfcer
    /// ignoring the parameter is a match rather than a guess.
    #[test]
    fn currency_style_is_inert() {
        let with_style = |style: i64| {
            render(
                &FormatHelper::Number {
                    decimals: 2,
                    separator_style: 0,
                    negative_style: 0,
                    currency_style: style,
                    currency: Vec::new(),
                    prepend_currency: true,
                },
                "1234.56",
                CommaPolicy::default(),
            )
        };
        assert_eq!(with_style(0), with_style(1));
        assert_eq!(text_of(&with_style(1)), "1,234.56");
    }

    /// ★ **An unsourced style code declines rather than guessing.**
    ///
    /// Mode 4 used to be the example here and is now MEASURED, so the
    /// test moved up to 5 — the first code nothing describes. That the
    /// boundary moved is the point: declining is a statement about the
    /// evidence, not a permanent property of a number.
    #[test]
    fn an_unsourced_style_code_declines_and_says_why() {
        let got = num("1234.56", 2, 5, 0, "", false);
        assert_eq!(
            got,
            FormatOutcome::UnknownStyle {
                parameter: "sepStyle",
                code: 5
            }
        );
        let why = got.decline_reason().expect("a decline explains itself");
        assert!(why.contains("sepStyle=5"), "{why}");
        assert!(
            why.contains("stored value"),
            "and says what is shown: {why}"
        );

        assert!(matches!(
            num("1", 2, 0, 9, "", false),
            FormatOutcome::UnknownStyle {
                parameter: "negStyle",
                ..
            }
        ));
    }

    /// The four negative styles are the two independent choices they are.
    #[test]
    fn the_four_negative_styles_are_bracket_crossed_with_red() {
        for (code, want_text, want_red) in [
            (0, "-1,234.56", false),
            (1, "-1,234.56", true),
            (2, "(1,234.56)", false),
            (3, "(1,234.56)", true),
        ] {
            let FormatOutcome::Rendered(f) = num("-1234.56", 2, 0, code, "", false) else {
                panic!("negStyle {code} must render");
            };
            assert_eq!(f.text, want_text, "negStyle {code}");
            assert_eq!(f.red, want_red, "negStyle {code} colour");
        }
    }

    /// Red is a property of a negative value, not of the style alone.
    #[test]
    fn a_positive_value_is_never_red_however_the_style_is_set() {
        let FormatOutcome::Rendered(f) = num("1234.56", 2, 0, 3, "", false) else {
            panic!("must render");
        };
        assert_eq!(f.text, "1,234.56", "and is not parenthesised either");
        assert!(!f.red);
    }

    /// The currency string is spliced on the side the boolean says, and a
    /// negative wraps the whole thing including the symbol.
    #[test]
    fn currency_is_spliced_on_the_named_side_and_wrapped_by_the_sign() {
        assert_eq!(text_of(&num("1234.5", 2, 0, 0, "$", true)), "$1,234.50");
        assert_eq!(
            text_of(&num("1234.5", 2, 0, 0, " USD", false)),
            "1,234.50 USD"
        );
        assert_eq!(
            text_of(&num("-1234.5", 2, 0, 2, "$", true)),
            "($1,234.50)",
            "the parentheses wrap the symbol, not just the digits"
        );
    }

    /// ★ **An empty or non-numeric value displays as nothing, not as 0.00.**
    ///
    /// A blank box that renders `0.00` reports an unfilled form as filled.
    #[test]
    fn an_empty_value_formats_to_nothing_rather_than_a_zero() {
        assert_eq!(text_of(&num("", 2, 0, 0, "", false)), "");
        assert_eq!(text_of(&num("   ", 2, 0, 0, "", false)), "");
        assert_eq!(text_of(&num("N/A", 2, 0, 0, "", false)), "");
        assert_eq!(
            text_of(&num("0", 2, 0, 0, "", false)),
            "0.00",
            "but a real stored zero DOES render, because it is a value"
        );
    }

    /// ★ **A percentage multiplies the stored value by 100.**
    ///
    /// The stored value is a fraction; the ×100 is display-only. Writing
    /// `8.5` into such a field would make it read `850%`.
    #[test]
    fn a_percentage_multiplies_the_stored_fraction_by_a_hundred() {
        let pct = |stored: &str, decimals: i64| {
            render(
                &FormatHelper::Percent {
                    decimals,
                    separator_style: 0,
                },
                stored,
                CommaPolicy::default(),
            )
        };
        assert_eq!(text_of(&pct("0.085", 1)), "8.5%");
        assert_eq!(text_of(&pct("0.5", 0)), "50%");
        assert_eq!(text_of(&pct("1", 2)), "100.00%");
        // ★ MEASURED, and it overturned what pdfcer had implemented. The one
        // available source has an empty percent field show a bare `%`; the
        // installed Acrobat showed `0.0%` for AFPercent_Format(1, 0) over an
        // empty value. Percent coerces an unreadable value to ZERO and
        // formats it, where AFNumber_Format leaves the field blank — a real
        // asymmetry between the two helpers.
        assert_eq!(text_of(&pct("", 1)), "0.0%");
        assert_eq!(text_of(&pct("", 2)), "0.00%");
        assert_eq!(
            text_of(&num("", 2, 0, 0, "", false)),
            "",
            "while a number field with the same empty value stays blank"
        );
    }

    /// Digit grouping is every three from the right, at any magnitude.
    #[test]
    fn digits_group_in_threes_from_the_right() {
        assert_eq!(
            text_of(&num("1234567.8", 1, 0, 0, "", false)),
            "1,234,567.8"
        );
        assert_eq!(text_of(&num("100", 0, 0, 0, "", false)), "100");
        assert_eq!(text_of(&num("1000", 0, 0, 0, "", false)), "1,000");
        assert_eq!(text_of(&num("0.5", 2, 0, 0, "", false)), "0.50");
    }

    /// Rounding is to the requested number of places, and an absurd request
    /// is clamped rather than refused.
    #[test]
    fn decimals_round_and_an_absurd_request_is_clamped() {
        assert_eq!(text_of(&num("1.005", 2, 1, 0, "", false)), "1.00");
        assert_eq!(text_of(&num("1.006", 2, 1, 0, "", false)), "1.01");
        assert_eq!(text_of(&num("2.5", 0, 1, 0, "", false)), "2");
        let clamped = num("1.5", 999, 1, 0, "", false);
        assert!(
            text_of(&clamped).len() < 40,
            "an absurd nDec is clamped, not refused: {}",
            text_of(&clamped)
        );
    }

    /// ★ **The phone mask ADAPTS to the digit count.**
    ///
    /// A fixed mask would mis-render every local number in the document.
    #[test]
    fn the_phone_mask_adapts_to_the_digit_count() {
        let sp = |stored: &str, selector: i64| {
            render(
                &FormatHelper::Special { selector },
                stored,
                CommaPolicy::default(),
            )
        };
        assert_eq!(text_of(&sp("5551234567", 2)), "(555) 123-4567");
        assert_eq!(text_of(&sp("1234567", 2)), "123-4567");
    }

    /// The fixed masks render, and a digit count that does not fit returns
    /// the stored value rather than a half-filled mask.
    #[test]
    fn the_fixed_masks_render_and_a_bad_length_returns_the_stored_value() {
        let sp = |stored: &str, selector: i64| {
            render(
                &FormatHelper::Special { selector },
                stored,
                CommaPolicy::default(),
            )
        };
        assert_eq!(text_of(&sp("12345", 0)), "12345");
        assert_eq!(text_of(&sp("123456789", 1)), "12345-6789");
        assert_eq!(text_of(&sp("123456789", 3)), "123-45-6789");
        assert_eq!(
            text_of(&sp("123", 0)),
            "123",
            "too few digits shows the value, not a truncated mask"
        );
        assert!(matches!(
            sp("12345", 7),
            FormatOutcome::UnknownStyle {
                parameter: "psf",
                ..
            }
        ));
    }

    /// Re-formatting an already-masked value does not compound separators.
    #[test]
    fn re_masking_an_already_masked_value_does_not_compound_separators() {
        let got = render(
            &FormatHelper::Special { selector: 2 },
            "(555) 123-4567",
            CommaPolicy::default(),
        );
        assert_eq!(text_of(&got), "(555) 123-4567");
    }

    /// ★ **A date renders through its predefined format when the stored
    /// value is unambiguous.**
    ///
    /// The token grammar is fully sourced; what was missing until now was a
    /// parse, and these are the shapes that cannot be read two ways.
    #[test]
    fn a_date_renders_when_the_stored_value_is_unambiguous() {
        let d = |index: i64, stored: &str| {
            render(
                &FormatHelper::Date { index },
                stored,
                CommaPolicy::default(),
            )
        };
        assert_eq!(text_of(&d(1, "2026-08-11")), "8/11/26");
        assert_eq!(text_of(&d(11, "2026-08-11")), "August 11, 2026");
        assert_eq!(
            text_of(&d(12, "2026-08-11 14:05:09")),
            "8/11/26 2:05 PM",
            "index 12 carries a time despite being a 'date' format"
        );
        assert_eq!(
            text_of(&render(
                &FormatHelper::Time { index: 0 },
                "2026-08-11 14:05:09",
                CommaPolicy::default()
            )),
            "14:05"
        );
        assert_eq!(
            text_of(&render(
                &FormatHelper::DateEx {
                    format: b"mm/dd/yyyy HH:MM".to_vec()
                },
                "2026-08-11 14:05:09",
                CommaPolicy::default()
            )),
            "08/11/2026 14:05",
            "and the case-sensitive month-versus-minutes pair survives the \
             whole path, not just the tokeniser"
        );
    }

    /// ★ **An ambiguous stored date declines, and says why.**
    ///
    /// `03/04/2026` names different days in different countries and the
    /// stored value settles nothing. Rendering a guess would be a confident
    /// wrong date, which on a form is indistinguishable from a right one.
    #[test]
    fn an_ambiguous_stored_date_declines_and_explains_itself() {
        let got = render(
            &FormatHelper::Date { index: 1 },
            "03/04/2026",
            CommaPolicy::default(),
        );
        assert_eq!(got, FormatOutcome::NotADate);
        let why = got.decline_reason().expect("explains itself");
        assert!(why.contains("different"), "{why}");
        assert!(
            why.contains("03/04/2026"),
            "and names the shape that is ambiguous: {why}"
        );
    }

    /// An empty date field formats to nothing, matching the number path — a
    /// blank box must not sprout a date.
    #[test]
    fn an_empty_date_field_formats_to_nothing() {
        assert_eq!(
            text_of(&render(
                &FormatHelper::Date { index: 1 },
                "   ",
                CommaPolicy::default()
            )),
            ""
        );
    }

    /// A predefined index no source describes declines by name rather than
    /// treating the index as a literal format string.
    #[test]
    fn an_unsourced_predefined_index_declines() {
        assert!(matches!(
            render(
                &FormatHelper::Date { index: 99 },
                "2026-08-11",
                CommaPolicy::default()
            ),
            FormatOutcome::UnknownStyle {
                parameter: "pdfFormat",
                code: 99
            }
        ));
        assert!(matches!(
            render(
                &FormatHelper::Time { index: 4 },
                "2026-08-11",
                CommaPolicy::default()
            ),
            FormatOutcome::UnknownStyle { .. }
        ));
    }

    /// ★ **Nothing here produces a value a caller could store.**
    ///
    /// The structural guard behind the format/value separation: a rendered
    /// result carries a display attribute that `/V` cannot hold, so storing
    /// one would mean visibly throwing it away first.
    #[test]
    fn a_formatted_result_is_structurally_not_a_stored_value() {
        let FormatOutcome::Rendered(f) = num("-1234.5", 2, 0, 1, "$", true) else {
            panic!("must render");
        };
        assert!(f.red, "the colour is not in the text");
        assert!(
            crate::form_script::calc::parse_number(&f.text, CommaPolicy::default()).is_none(),
            "and the text does not parse back as a number: {:?}",
            f.text
        );
    }
}
