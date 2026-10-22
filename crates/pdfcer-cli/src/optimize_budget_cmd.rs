//! `optimize --target-size`: parse the size and report the budget search.

use std::fmt::Write as _;

use pdfcer_core::optimize::budget::{BudgetAttempt, BudgetFit};

/// What the budget search did, for the report.
pub(crate) struct BudgetSummary {
    pub(crate) target: u64,
    pub(crate) step: usize,
    pub(crate) met: bool,
    pub(crate) attempts: Vec<BudgetAttempt>,
}

impl BudgetSummary {
    /// The report's view of `fit`, searched against `target` bytes.
    pub(crate) fn of(fit: &BudgetFit, target: u64) -> Self {
        Self {
            target,
            step: fit.step,
            met: fit.met,
            attempts: fit.attempts.clone(),
        }
    }
}

/// Parse `--target-size`: a byte count with an optional unit. `KB`/`MB`/`GB`
/// are decimal (1000), `KiB`/`MiB`/`GiB` binary (1024); `B` or no unit is
/// bytes. Case-insensitive; a fraction is allowed with a unit (`1.5MB`).
pub(crate) fn parse_size(s: &str) -> Result<u64, String> {
    let t = s.trim();
    let split = t
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(t.len());
    let (number, unit) = t.split_at(split);
    let scale: u64 = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "kb" | "k" => 1_000,
        "mb" | "m" => 1_000_000,
        "gb" | "g" => 1_000_000_000,
        "kib" => 1 << 10,
        "mib" => 1 << 20,
        "gib" => 1 << 30,
        other => {
            return Err(format!(
                "unknown unit {other:?}; use B, KB, MB, GB, KiB, MiB or GiB"
            ));
        }
    };
    let value: f64 = number
        .parse()
        .map_err(|_| format!("{s:?} is not a size like 2MB, 500KiB or 150000"))?;
    let bytes = (value * scale as f64).floor();
    if !(1.0..=u64::MAX as f64).contains(&bytes) {
        return Err(format!("{s:?} must be at least 1 byte"));
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    // Checked in range just above.
    Ok(bytes as u64)
}

/// What step `a` imposed, in words.
fn step_words(a: &BudgetAttempt) -> String {
    match (a.quality, a.ppi) {
        (None, _) => "your settings as given".to_owned(),
        (Some(q), None) => format!("JPEG quality <= {q}"),
        (Some(q), Some(p)) => format!("JPEG quality <= {q}, <= {p} ppi"),
    }
}

/// The report's budget section.
pub(crate) fn budget_text(b: &BudgetSummary) -> String {
    let mut out = format!("budget: target {} B\n", b.target);
    for a in &b.attempts {
        let verdict = if a.bytes <= b.target { "fits" } else { "over" };
        let _ = writeln!(
            out,
            "  step {:>2} ({}): {} B, {verdict}",
            a.step,
            step_words(a),
            a.bytes
        );
    }
    let chosen = b.attempts.iter().find(|a| a.step == b.step);
    let words = chosen.map_or_else(String::new, step_words);
    if b.met {
        let _ = writeln!(
            out,
            "budget: met at step {} ({words}), the least lossy step that fits",
            b.step
        );
    } else {
        let _ = writeln!(
            out,
            "budget: NOT met; the lossiest step ({words}) is the closest and is what was kept (exit 15)"
        );
    }
    out
}

/// The JSON report's `budget` value.
pub(crate) fn budget_json(b: &BudgetSummary) -> String {
    let opt = |v: Option<u16>| v.map_or_else(|| "null".to_owned(), |v| v.to_string());
    let attempts: Vec<String> = b
        .attempts
        .iter()
        .map(|a| {
            format!(
                "{{\"step\":{},\"quality\":{},\"ppi\":{},\"bytes\":{}}}",
                a.step,
                opt(a.quality.map(u16::from)),
                opt(a.ppi),
                a.bytes
            )
        })
        .collect();
    format!(
        "{{\"target_bytes\":{},\"met\":{},\"step\":{},\"attempts\":[{}]}}",
        b.target,
        b.met,
        b.step,
        attempts.join(",")
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::parse_size;

    #[test]
    fn sizes_parse_with_decimal_and_binary_units() {
        assert_eq!(parse_size("150000").unwrap(), 150_000);
        assert_eq!(parse_size("2MB").unwrap(), 2_000_000);
        assert_eq!(parse_size("1.5 mb").unwrap(), 1_500_000);
        assert_eq!(parse_size("500KiB").unwrap(), 512_000);
        assert_eq!(parse_size("1GiB").unwrap(), 1 << 30);
        assert!(parse_size("0").is_err());
        assert!(parse_size("2 furlongs").is_err());
        assert!(parse_size("MB").is_err());
    }
}
