//! Port of the automatic column width fitting for the Details view
//! ("AutoFitColumns").
//!
//! C# source:
//! - `Files.App/Actions/Display/AutoFitColumnsAction.cs` — the command,
//!   which calls `DetailsLayoutPage.AutoFitColumns()`.
//! - `Files.App/Views/Layouts/DetailsLayoutPage.xaml.cs`
//!   (`AutoFitColumns`, `ResizeColumnToFit`, `MeasureColumnEstimate`,
//!   `MeasureTextColumnEstimate`).
//! - `Files.App/Data/Items/DetailsLayoutColumnItem.cs` — default bounds
//!   (`NormalMinLength = 50`, `NormalMaxLength = 800`).
//!
//! # What the C# does
//!
//! `ResizeColumnToFit` computes, for each column, `maxItemLength` = the
//! length (in number of characters) of the longest text among the items:
//!
//! ```csharp
//! 2 => FileList.Items.Cast<ListedItem>().Select(x => x.Name?.Length ?? 0).Max(), // file name column
//! ```
//!
//! Then `MeasureTextColumnEstimate` estimates a **width per letter** by
//! actually measuring (DirectWrite via `TextBlock.Measure`) the 5 longest
//! texts, and dividing the measured width by their length:
//!
//! ```csharp
//! return sampleTb.DesiredSize.Width / Math.Max(1, tb.Text.Length);
//! // ...
//! // Take weighted avg between mean and max since width is an estimate
//! var weightedAvg = (widthPerLetter.Average() + widthPerLetter.Max()) / 2;
//! return weightedAvg * maxItemLength;
//! ```
//!
//! Finally, `ResizeColumnToFit` applies the bounds and padding:
//!
//! ```csharp
//! if (columnToResize == 2) // file name column
//!     columnSizeToFit += 20;
//!
//! var minFitLength = Math.Max(columnSizeToFit, column.NormalMinLength);      // >= 50
//! var maxFitLength = Math.Min(minFitLength + 36, column.NormalMaxLength);    // + padding, <= 800
//!
//! column.UserLength = new GridLength(maxFitLength, GridUnitType.Pixel);
//! ```
//!
//! Key values (quotes):
//! - `NormalMinLength = 50` (default of `DetailsLayoutColumnItem`).
//! - `NormalMaxLength = 800` (default; overridden to 1000 for the name,
//!   500 for paths, 80 for status…).
//! - Padding `+ 36`: C# comment "36 to account for SortIcon & padding".
//! - Extra `+ 20` for the name column (column 2).
//!
//! # Deliberate port differences
//!
//! The port doesn't have DirectWrite here: we reuse the port's text width
//! estimate (`crate::ui::approx_text_width`, ~`0.52 * font_px` per
//! character) instead of measuring each `TextBlock`. We take the column
//! header into account (the C# doesn't do this explicitly, since the
//! header is a separate grid column, but the `+ 36` padding precisely
//! covers the header's sort icon).

// NOT WIRED YET. Ported column auto-fit of the details view (double-click on a column divider); waits for the details header to call it.
// The allow is scoped to this file and must go once it is wired.
#![allow(dead_code)]

/// Minimum column length — `DetailsLayoutColumnItem.NormalMinLength`
/// (default) on the C# side.
pub const NORMAL_MIN_LENGTH: f32 = 50.0;

/// Maximum column length — `DetailsLayoutColumnItem.NormalMaxLength`
/// (default) on the C# side.
pub const NORMAL_MAX_LENGTH: f32 = 800.0;

/// Padding added to the fitted width: "36 to account for SortIcon &
/// padding" (`ResizeColumnToFit`).
pub const FIT_PADDING: f32 = 36.0;

/// Text width estimate, consistent with the rest of the port.
///
/// Reuses `crate::ui::approx_text_width` (~`0.52 * font_px` per
/// character), the equivalent of the DirectWrite measurement used by
/// `MeasureTextColumnEstimate` on the C# side.
fn text_width(text: &str, font_px: f32) -> f32 {
    crate::ui::approx_text_width(text, font_px)
}

/// Computes the ideal width of a column to "fit content".
///
/// = `max(header width, max cell width)`, floored at
/// `NORMAL_MIN_LENGTH` then increased by `FIT_PADDING` and capped at
/// `NORMAL_MAX_LENGTH` — faithful to `ResizeColumnToFit`:
/// `Math.Min(Math.Max(fit, min) + 36, max)`.
pub fn autofit_width(header: &str, cell_texts: &[String], font_px: f32) -> f32 {
    // Content width = max(header, widest cell).
    let mut content = text_width(header, font_px);
    for cell in cell_texts {
        let w = text_width(cell, font_px);
        if w > content {
            content = w;
        }
    }

    // minFitLength = Math.Max(columnSizeToFit, NormalMinLength)
    let min_fit = content.max(NORMAL_MIN_LENGTH);

    // maxFitLength = Math.Min(minFitLength + 36, NormalMaxLength)
    (min_fit + FIT_PADDING).min(NORMAL_MAX_LENGTH)
}

/// Applies [`autofit_width`] to all columns.
///
/// `rows` is a list of rows, each row being a vector of cells aligned
/// with `headers`. Returns the ideal width of each column, in the order
/// of `headers` (equivalent to the `AutoFitColumns` loop which calls
/// `ResizeColumnToFit` for each column).
pub fn autofit_all(headers: &[String], rows: &[Vec<String>], font_px: f32) -> Vec<f32> {
    headers
        .iter()
        .enumerate()
        .map(|(col, header)| {
            // Collects the cells of this column across all rows.
            let cells: Vec<String> = rows
                .iter()
                .filter_map(|row| row.get(col).cloned())
                .collect();
            autofit_width(header, &cells, font_px)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FONT: f32 = 14.0;

    /// An empty column (short header, no cell) stays at minimum + padding.
    #[test]
    fn respecte_minimum_et_padding() {
        let w = autofit_width("A", &[], FONT);
        assert_eq!(w, NORMAL_MIN_LENGTH + FIT_PADDING);
    }

    /// The width grows with the longest cell text.
    #[test]
    fn croit_avec_le_texte_le_plus_long() {
        let court = autofit_width(
            "Nom",
            &["a.txt".to_string(), "b.txt".to_string()],
            FONT,
        );
        let long = autofit_width(
            "Nom",
            &[
                "a.txt".to_string(),
                "un_nom_de_fichier_vraiment_tres_long.txt".to_string(),
            ],
            FONT,
        );
        assert!(long > court, "long={long} devrait dépasser court={court}");
    }

    /// The header imposes a floor: a wide header widens the column even
    /// if all cells are short.
    #[test]
    fn respecte_l_en_tete() {
        let entete_court = autofit_width("N", &["x".to_string()], FONT);
        let entete_long = autofit_width(
            "Un en-tête de colonne particulièrement long",
            &["x".to_string()],
            FONT,
        );
        assert!(entete_long > entete_court);
    }

    /// The `NormalMaxLength` cap is never exceeded.
    #[test]
    fn plafonne_au_maximum() {
        let enorme = "x".repeat(10_000);
        let w = autofit_width("h", &[enorme], FONT);
        assert_eq!(w, NORMAL_MAX_LENGTH);
    }

    /// The width increases monotonically with content length, as long as
    /// we stay under the cap.
    #[test]
    fn monotone_avant_plafond() {
        let a = autofit_width("h", &["aaaaaaaaaa".to_string()], FONT);
        let b = autofit_width("h", &["aaaaaaaaaaaaaaaaaaaa".to_string()], FONT);
        assert!(b > a);
        assert!(b <= NORMAL_MAX_LENGTH);
    }

    /// `autofit_all` processes each column independently and preserves order.
    #[test]
    fn autofit_all_par_colonne() {
        let headers = vec!["Nom".to_string(), "Taille".to_string()];
        let rows = vec![
            vec!["fichier_avec_un_nom_long.txt".to_string(), "1 Ko".to_string()],
            vec!["a.txt".to_string(), "12 Mo".to_string()],
        ];
        let widths = autofit_all(&headers, &rows, FONT);
        assert_eq!(widths.len(), 2);
        // The "Nom" column (longer content) is wider than "Taille".
        assert!(widths[0] > widths[1]);
        // Each one respects the bounds.
        for w in widths {
            assert!(w >= NORMAL_MIN_LENGTH + FIT_PADDING || w == NORMAL_MAX_LENGTH);
            assert!(w <= NORMAL_MAX_LENGTH);
        }
    }
}
