//! The counters that say a render differs from what the file asks.

use crate::interpret::Diagnostics;

impl Diagnostics {
    /// Keys of every DIVERGENCE counter, in the order
    /// [`Diagnostics::divergences`] yields them.
    ///
    /// Each key is the one `pdfcer render`'s metrics line prints, so a
    /// consumer can report a counter under the name a script sees. A
    /// DIVERGENCE counter means the page was painted differently from what
    /// the document asks; a non-zero value is a reason the screen may not be
    /// what a press or another conforming reader shows.
    pub const DIVERGENCE_KEYS: [&'static str; 15] = [
        "overprint_refused",
        "overprint_images_unsupported",
        "overprint_process_images_unsupported",
        "overprint_shadings_unsupported",
        "cmyk_buffer_refused",
        "blends_in_wrong_space",
        "cmyk_groups_approximated",
        "soft_masks_ignored",
        "soft_mask_transfer_ignored",
        "blend_modes_ignored",
        "transparency_groups_flattened",
        "color.spaces_unresolved",
        "color.tint_transform_not_applied",
        "color.patterns_unpainted",
        "shading.refused",
    ];

    /// Every DIVERGENCE counter as `(key, value)`, zero values included.
    ///
    /// Keys are [`Diagnostics::DIVERGENCE_KEYS`], in that order. A counter
    /// added later appears here and in that list, so a consumer that walks
    /// this iterator reports it without code changes.
    ///
    /// ```
    /// let d = pdfcer_render::Diagnostics::default();
    /// assert!(d.divergences().all(|(_, n)| n == 0));
    /// assert_eq!(d.divergences().count(), 15);
    /// ```
    pub fn divergences(&self) -> impl Iterator<Item = (&'static str, u64)> {
        let values: [u64; 15] = [
            widen(self.overprint_refused),
            widen(self.overprint_images_unsupported),
            widen(self.overprint_process_images_unsupported),
            widen(self.overprint_shadings_unsupported),
            widen(self.cmyk_buffer_refused),
            widen(self.blends_in_wrong_space),
            self.cmyk_groups_approximated,
            widen(self.soft_masks_ignored),
            widen(self.soft_mask_transfer_ignored),
            widen(self.blend_modes_ignored),
            widen(self.transparency_groups_flattened),
            widen(self.color.spaces_unresolved),
            widen(self.color.tint_transform_not_applied),
            widen(self.color.patterns_unpainted),
            widen(self.shading.refused),
        ];
        Self::DIVERGENCE_KEYS.into_iter().zip(values)
    }
}

fn widen(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The keys the CLI metrics table classes as DIVERGENCE: the second
    /// backticked column of every `//! |` row whose description says so.
    fn cli_table_divergence_keys() -> Vec<String> {
        include_str!("../../pdfcer-cli/src/main.rs")
            .lines()
            .filter(|l| l.starts_with("//! | `") && l.contains("DIVERGENCE"))
            .filter_map(|l| l.split('`').nth(3).map(str::to_owned))
            .collect()
    }

    #[test]
    fn the_keys_equal_the_cli_tables_divergence_rows() {
        let mut ours: Vec<String> = Diagnostics::DIVERGENCE_KEYS
            .iter()
            .map(|k| (*k).to_owned())
            .collect();
        let mut cli = cli_table_divergence_keys();
        ours.sort();
        cli.sort();
        assert_eq!(ours, cli);
    }

    #[test]
    fn each_key_reads_its_own_counter() {
        let mut d = Diagnostics {
            overprint_refused: 1,
            cmyk_groups_approximated: 7,
            ..Diagnostics::default()
        };
        d.color.patterns_unpainted = 3;
        d.shading.refused = 5;
        let got: Vec<(&str, u64)> = d.divergences().filter(|(_, n)| *n != 0).collect();
        assert_eq!(
            got,
            [
                ("overprint_refused", 1),
                ("cmyk_groups_approximated", 7),
                ("color.patterns_unpainted", 3),
                ("shading.refused", 5),
            ]
        );
    }
}
