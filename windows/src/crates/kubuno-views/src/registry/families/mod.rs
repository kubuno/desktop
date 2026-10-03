//! The component families beyond the five phase-2a examples, one file each.
//!
//! Each family is behind its own Cargo feature (`family-<name>`), so the
//! families can be developed in parallel without one unfinished file breaking
//! the others' builds; the `all-families` feature (on by default once every
//! family is integrated) turns them all on.

#[cfg(feature = "family-choice")]
pub mod choice;
// The non-visual components (`<Timer>`, EVT-7b): always built.
pub mod components;
// The ribbon family (vskubuno/docs/RIBBON.md): always built.
pub mod ribbon;
// The menu family (vskubuno/docs/MENUS.md): always built.
pub mod menus;
#[cfg(feature = "family-containers")]
pub mod containers;
#[cfg(feature = "family-data")]
pub mod data;
#[cfg(feature = "family-display")]
pub mod display;
#[cfg(feature = "family-docking")]
pub mod docking;
#[cfg(feature = "family-items")]
pub mod items;
#[cfg(feature = "family-navigation")]
pub mod navigation;
#[cfg(feature = "family-media")]
pub mod media;
#[cfg(feature = "family-overlays")]
pub mod overlays;
#[cfg(feature = "family-layout")]
pub mod layout;
#[cfg(feature = "family-text")]
pub mod text;

use super::ComponentMeta;

/// The enabled families' component tables, in a stable order.
pub const ALL_FAMILIES: &[&[ComponentMeta]] = &[
    #[cfg(feature = "family-display")]
    display::ALL,
    #[cfg(feature = "family-choice")]
    choice::ALL,
    #[cfg(feature = "family-text")]
    text::ALL,
    #[cfg(feature = "family-containers")]
    containers::ALL,
    #[cfg(feature = "family-data")]
    data::ALL,
    #[cfg(feature = "family-docking")]
    docking::ALL,
    #[cfg(feature = "family-items")]
    items::ALL,
    #[cfg(feature = "family-navigation")]
    navigation::ALL,
    #[cfg(feature = "family-media")]
    media::ALL,
    #[cfg(feature = "family-overlays")]
    overlays::ALL,
    #[cfg(feature = "family-layout")]
    layout::ALL,
    components::ALL,
    ribbon::ALL,
    menus::ALL,
];
