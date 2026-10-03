//! PURE primitives of the Omnibar (mode-driven address pill bar).
//! Port of `Files.App.Controls/Omnibar/` (FLAT folder). This `mod.rs` is just
//! GLUE: it declares the mirror sub-files and flatly re-exports the whole
//! public API (`omnibar::X` stays unchanged for consumers).

pub mod event_args;
pub mod i_omnibar_text_member_path_provider;
pub mod omnibar;
pub mod omnibar_mode;
pub mod omnibar_mode_separator;
pub mod omnibar_text_change_reason;

pub use omnibar::{
    content_padding, draw, mode_button_rects, OmnibarView, BORDER_FOCUSED, BORDER_UNFOCUSED,
    CONTENT_RIGHT_MARGIN, CORNER_RADIUS, HEIGHT, MODES_HOST_PADDING_X,
};
pub use omnibar_mode::{
    OmnibarMode, MODE_BUTTON_MARGIN, MODE_CLICK_AREA_WIDTH, MODE_CORNER_RADIUS, MODE_HEIGHT,
    MODE_SLOT_WIDTH,
};
pub use omnibar_mode_separator::{
    separator_rect, SEPARATOR_HEIGHT, SEPARATOR_PADDING_X, SEPARATOR_SLOT_WIDTH, SEPARATOR_WIDTH,
};
