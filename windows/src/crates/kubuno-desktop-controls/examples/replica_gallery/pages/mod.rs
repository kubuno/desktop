//! One page per control family, mirroring the reference sheets one for one.
//!
//! The order, the group captions, the control states and the strings are the
//! ones in `tools/winforms-ref/gallery/Families.cs`, so
//! `C:\kubuno-build\winforms-ref\shots\NN-family.png` and the matching page here
//! can be placed side by side and read as the same sheet.
//!
//! `10-grid` has no page: `DataGridView` and `PropertyGrid` are explicitly out
//! of scope for this wave (`docs/AGENT_BRIEF.md`, §7), and the brief forbids
//! stubbing them.

use crate::sheet::Sheet;

mod buttons;
mod containers;
mod datetime;
mod labels;
mod lists;
mod range;
mod text;
mod toolstrip;
mod views;

/// A page: the reference sheet's file name, the label its switcher tab carries,
/// and the builder that produces the sheet.
pub struct Page {
    /// The reference sheet's stem — `--page` takes this, or its family part.
    pub id:    &'static str,
    /// The switcher's caption (French, like the rest of the product).
    pub label: &'static str,
    pub build: fn() -> Sheet,
}

pub const PAGES: &[Page] = &[
    Page { id: "01-buttonbase", label: "Boutons", build: buttons::build },
    Page { id: "02-textboxbase", label: "Saisie", build: text::build },
    Page { id: "03-listcontrol", label: "Listes", build: lists::build },
    Page { id: "04-containers", label: "Conteneurs", build: containers::build },
    Page { id: "05-labels", label: "Étiquettes", build: labels::build },
    Page { id: "06-range", label: "Plages", build: range::build },
    Page { id: "07-datetime", label: "Dates", build: datetime::build },
    Page { id: "08-views", label: "Vues", build: views::build },
    Page { id: "09-toolstrip", label: "Barres", build: toolstrip::build },
];

/// Resolves `--page <name>`: the full sheet stem (`01-buttonbase`), the family
/// part alone (`buttonbase`), or the 1-based index (`1`).
pub fn find(name: &str) -> Option<usize> {
    let key = name.trim().to_ascii_lowercase();
    PAGES
        .iter()
        .position(|p| {
            p.id == key || p.id.split_once('-').is_some_and(|(n, fam)| fam == key || n == key)
        })
        .or_else(|| {
            key.parse::<usize>()
                .ok()
                .filter(|i| (1..=PAGES.len()).contains(i))
                .map(|i| i - 1)
        })
}
