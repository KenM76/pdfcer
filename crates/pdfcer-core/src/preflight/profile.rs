//! Profiles: named sets of rules, the built-in ones, and their JSON form.
//!
//! The JSON form is the one a shell saves an edited copy in:
//!
//! ```json
//! {"name": "Digital printing", "description": "...",
//!  "rules": [{"id": "image-low-ppi", "severity": "warning",
//!             "check": "image-below-ppi", "min_ppi": 150}]}
//! ```
//!
//! Check names and their parameters are listed on [`Profile::from_json`].

use pdfcer_json::Json;

use super::{Check, ColourFamily, PdfxLevel, Severity};
use crate::PdfVersion;

/// One rule: a check, the id findings carry, and their severity.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileRule {
    /// Stable id, unique in its profile (`image-low-ppi`).
    pub id: String,
    /// What is checked.
    pub check: Check,
    /// The severity of every finding the rule produces.
    pub severity: Severity,
}

impl ProfileRule {
    /// A rule from its parts.
    #[must_use]
    pub fn new(id: impl Into<String>, check: Check, severity: Severity) -> Self {
        Self {
            id: id.into(),
            check,
            severity,
        }
    }
}

/// A named set of rules.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    /// The name a person picks it by.
    pub name: String,
    /// What it is for.
    pub description: String,
    /// The rules, run in order.
    pub rules: Vec<ProfileRule>,
}

/// Why profile JSON was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ProfileError {
    /// The bytes are not JSON.
    #[error("profile is not JSON: {0}")]
    Json(String),
    /// A required member is missing or has the wrong type.
    #[error("profile member `{0}` is missing or has the wrong type")]
    Member(String),
    /// A rule names a check this version does not know.
    #[error("rule `{rule}` names an unknown check `{check}`")]
    UnknownCheck {
        /// The rule's id.
        rule: String,
        /// The check name it gave.
        check: String,
    },
    /// A value is out of range or unrecognised.
    #[error("rule `{rule}`: {why}")]
    BadValue {
        /// The rule's id.
        rule: String,
        /// What is wrong.
        why: String,
    },
    /// Two rules share an id.
    #[error("two rules share the id `{0}`")]
    DuplicateId(String),
}

impl Profile {
    /// The names [`Self::builtin`] accepts.
    pub const BUILTIN_NAMES: [&'static str; 8] = [
        "digital-printing",
        "pdfua-1",
        "pdfua-2",
        "pdfx-1a-2001",
        "pdfx-1a-2003",
        "pdfx-3-2002",
        "pdfx-3-2003",
        "pdfx-4",
    ];

    /// A built-in profile by its [`Self::BUILTIN_NAMES`] name.
    #[must_use]
    pub fn builtin(name: &str) -> Option<Self> {
        match name {
            "digital-printing" => Some(Self::digital_printing()),
            "pdfua-1" => Some(Self::pdfua(super::PdfuaPart::Ua1)),
            "pdfua-2" => Some(Self::pdfua(super::PdfuaPart::Ua2)),
            other => PdfxLevel::ALL
                .into_iter()
                .find(|l| l.profile_name() == other)
                .map(Self::pdfx),
        }
    }

    /// Checks for a digital press: embedded fonts, pictures of at least
    /// 150 ppi, and notes on RGB, overprint, missing trim boxes and
    /// encryption.
    #[must_use]
    pub fn digital_printing() -> Self {
        let rgb = vec![
            ColourFamily::DeviceRgb,
            ColourFamily::CalRgb,
            ColourFamily::IccRgb,
        ];
        Self {
            name: "Digital printing".to_owned(),
            description: "Fonts embedded, pictures at 150 ppi or more; RGB, overprint, \
                          missing trim boxes and encryption noted."
                .to_owned(),
            rules: vec![
                ProfileRule::new("font-not-embedded", Check::FontNotEmbedded, Severity::Error),
                ProfileRule::new(
                    "image-low-ppi",
                    Check::ImageBelowPpi { min_ppi: 150.0 },
                    Severity::Warning,
                ),
                ProfileRule::new("encrypted", Check::Encrypted, Severity::Warning),
                ProfileRule::new(
                    "rgb-colour",
                    Check::ColourSpaceUsed { families: rgb },
                    Severity::Info,
                ),
                ProfileRule::new("overprint", Check::OverprintUsed, Severity::Info),
                ProfileRule::new("no-trim-box", Check::NoTrimBox, Severity::Info),
            ],
        }
    }

    /// PDF/UA conformance (ISO 14289-1 or -2) as one rule; see
    /// [`super::validate_pdfua`].
    #[must_use]
    pub fn pdfua(part: super::PdfuaPart) -> Self {
        let n = part.number();
        Self {
            name: format!("PDF/UA-{n}"),
            description: format!(
                "{} accessibility: machine checks, plus the checks a person must make.",
                part.standard()
            ),
            rules: vec![ProfileRule::new(
                format!("pdfua-{n}"),
                Check::PdfUa { part },
                Severity::Error,
            )],
        }
    }

    /// The profile as JSON, in the form [`Self::from_json`] reads.
    #[must_use]
    pub fn to_json(&self) -> String {
        let rules: Vec<String> = self.rules.iter().map(rule_json).collect();
        format!(
            "{{\n  \"name\": {},\n  \"description\": {},\n  \"rules\": [\n    {}\n  ]\n}}\n",
            quote(&self.name),
            quote(&self.description),
            rules.join(",\n    ")
        )
    }

    /// Read a profile from JSON.
    ///
    /// Each rule has `id`, `severity` (`error`, `warning`, `info`) and
    /// `check`, plus the check's parameters:
    ///
    /// | `check` | parameters |
    /// |---|---|
    /// | `image-below-ppi` | `min_ppi`: number > 0 |
    /// | `font-not-embedded` | none |
    /// | `no-output-intent` | none |
    /// | `no-trim-box` | none |
    /// | `colour-space-used` | `families`: array of [`ColourFamily::as_str`] names |
    /// | `spot-not-allowed` | `allowed`: array of ink names |
    /// | `overprint-used` | none |
    /// | `stroke-below` | `min_width_pt`: number >= 0 |
    /// | `transparency-used` | none |
    /// | `encrypted` | none |
    /// | `version-above` | `max`: `"1.7"`-style string |
    /// | `pdfx-requirement` | `level`: [`PdfxLevel::as_str`] name; `row`: an inventory row id that applies to it |
    ///
    /// # Errors
    ///
    /// [`ProfileError`] naming the member or rule that is wrong. Unknown
    /// members are ignored; an unknown check is refused.
    pub fn from_json(bytes: &[u8]) -> Result<Self, ProfileError> {
        let root = pdfcer_json::parse(bytes).map_err(|e| ProfileError::Json(e.to_string()))?;
        let text = |key: &str| root.get(key).and_then(Json::as_str).map(str::to_owned);
        let name = text("name").ok_or_else(|| ProfileError::Member("name".into()))?;
        let description = text("description").unwrap_or_default();
        let items = root
            .get("rules")
            .and_then(Json::as_array)
            .ok_or_else(|| ProfileError::Member("rules".into()))?;
        let mut rules: Vec<ProfileRule> = Vec::with_capacity(items.len());
        for item in items {
            let rule = parse_rule(item)?;
            if rules.iter().any(|r| r.id == rule.id) {
                return Err(ProfileError::DuplicateId(rule.id));
            }
            rules.push(rule);
        }
        Ok(Self {
            name,
            description,
            rules,
        })
    }
}

fn parse_rule(item: &Json) -> Result<ProfileRule, ProfileError> {
    let id = item
        .get("id")
        .and_then(Json::as_str)
        .ok_or_else(|| ProfileError::Member("rules[].id".into()))?
        .to_owned();
    let bad = |why: &str| ProfileError::BadValue {
        rule: id.clone(),
        why: why.to_owned(),
    };
    let severity = item
        .get("severity")
        .and_then(Json::as_str)
        .and_then(Severity::from_name)
        .ok_or_else(|| bad("severity must be error, warning or info"))?;
    let name = item
        .get("check")
        .and_then(Json::as_str)
        .ok_or_else(|| ProfileError::Member(format!("rules[{id}].check")))?;
    let check = parse_check(name, &id, item)?;
    Ok(ProfileRule {
        id,
        check,
        severity,
    })
}

/// The `check` member and its parameters.
fn parse_check(name: &str, id: &str, item: &Json) -> Result<Check, ProfileError> {
    let bad = |why: &str| ProfileError::BadValue {
        rule: id.to_owned(),
        why: why.to_owned(),
    };
    let strings = |key: &str| -> Option<Vec<String>> {
        item.get(key)?
            .as_array()?
            .iter()
            .map(|v| v.as_str().map(str::to_owned))
            .collect()
    };
    let check = match name {
        "image-below-ppi" => Check::ImageBelowPpi {
            min_ppi: number(item, "min_ppi", |v| v > 0.0)
                .ok_or_else(|| bad("min_ppi must be a number above 0"))?,
        },
        "font-not-embedded" => Check::FontNotEmbedded,
        "no-output-intent" => Check::NoOutputIntent,
        "no-trim-box" => Check::NoTrimBox,
        "colour-space-used" => {
            let names =
                strings("families").ok_or_else(|| bad("families must be an array of names"))?;
            let families = names
                .iter()
                .map(|n| {
                    ColourFamily::from_name(n)
                        .ok_or_else(|| bad(&format!("unknown colour family `{n}`")))
                })
                .collect::<Result<_, _>>()?;
            Check::ColourSpaceUsed { families }
        }
        "spot-not-allowed" => Check::SpotNotAllowed {
            allowed: strings("allowed")
                .ok_or_else(|| bad("allowed must be an array of ink names"))?,
        },
        "overprint-used" => Check::OverprintUsed,
        "stroke-below" => Check::StrokeBelow {
            min_width_pt: number(item, "min_width_pt", |v| v >= 0.0)
                .ok_or_else(|| bad("min_width_pt must be a number of at least 0"))?,
        },
        "transparency-used" => Check::TransparencyUsed,
        "encrypted" => Check::Encrypted,
        "pdfua" => {
            let part = item
                .get("part")
                .and_then(Json::as_u32)
                .and_then(|n| u8::try_from(n).ok())
                .and_then(super::PdfuaPart::from_number)
                .ok_or_else(|| bad("part must be 1 or 2"))?;
            Check::PdfUa { part }
        }
        "version-above" => {
            let max = item
                .get("max")
                .and_then(Json::as_str)
                .and_then(parse_version)
                .ok_or_else(|| bad("max must be a version such as \"1.7\""))?;
            Check::VersionAbove { max }
        }
        "pdfx-requirement" => pdfx_requirement(item).ok_or_else(|| bad(PDFX_BAD))?,
        other => {
            return Err(ProfileError::UnknownCheck {
                rule: id.to_owned(),
                check: other.to_owned(),
            });
        }
    };
    Ok(check)
}

const PDFX_BAD: &str = "level must be a PDF/X level and row an inventory row that applies to it";

fn pdfx_requirement(item: &Json) -> Option<Check> {
    let level = PdfxLevel::from_name(item.get("level")?.as_str()?)?;
    let row = super::pdfx::row_id(item.get("row")?.as_str()?, level)?;
    Some(Check::PdfxRequirement { level, row })
}

/// `item[key]` as a finite number that `ok` accepts.
fn number(item: &Json, key: &str, ok: impl Fn(f64) -> bool) -> Option<f64> {
    item.get(key)
        .and_then(Json::as_f64)
        .filter(|&v| v.is_finite() && ok(v))
}

fn parse_version(s: &str) -> Option<PdfVersion> {
    let (major, minor) = s.split_once('.')?;
    Some(PdfVersion {
        major: major.parse().ok()?,
        minor: minor.parse().ok()?,
    })
}

fn rule_json(rule: &ProfileRule) -> String {
    let (name, params) = match &rule.check {
        Check::ImageBelowPpi { min_ppi } => {
            ("image-below-ppi", format!(", \"min_ppi\": {min_ppi}"))
        }
        Check::FontNotEmbedded => ("font-not-embedded", String::new()),
        Check::NoOutputIntent => ("no-output-intent", String::new()),
        Check::NoTrimBox => ("no-trim-box", String::new()),
        Check::ColourSpaceUsed { families } => {
            let names: Vec<String> = families.iter().map(|f| quote(f.as_str())).collect();
            (
                "colour-space-used",
                format!(", \"families\": [{}]", names.join(", ")),
            )
        }
        Check::SpotNotAllowed { allowed } => {
            let names: Vec<String> = allowed.iter().map(|n| quote(n)).collect();
            (
                "spot-not-allowed",
                format!(", \"allowed\": [{}]", names.join(", ")),
            )
        }
        Check::OverprintUsed => ("overprint-used", String::new()),
        Check::StrokeBelow { min_width_pt } => (
            "stroke-below",
            format!(", \"min_width_pt\": {min_width_pt}"),
        ),
        Check::TransparencyUsed => ("transparency-used", String::new()),
        Check::Encrypted => ("encrypted", String::new()),
        Check::VersionAbove { max } => ("version-above", format!(", \"max\": \"{max}\"")),
        Check::PdfUa { part } => ("pdfua", format!(", \"part\": {}", part.number())),
        Check::PdfxRequirement { level, row } => (
            "pdfx-requirement",
            format!(
                ", \"level\": \"{}\", \"row\": {}",
                level.as_str(),
                quote(row)
            ),
        ),
    };
    format!(
        "{{\"id\": {}, \"severity\": \"{}\", \"check\": \"{name}\"{params}}}",
        quote(&rule.id),
        rule.severity.as_str()
    )
}

/// A JSON string literal (RFC 8259 §7).
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
