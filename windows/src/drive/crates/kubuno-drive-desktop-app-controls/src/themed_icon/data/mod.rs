//! `ThemedIcon` data types (mirrors ThemedIcon/Data/).
//!
//! Pure glue: declares the data enum/collection submodules and flat-re-exports
//! them to preserve the `themed_icon::data::X` path.

pub mod themed_icon_color_type;
pub mod themed_icon_layer_type;
pub mod themed_icon_layers;
pub mod themed_icon_toggle_behaviors;
pub mod themed_icon_types;

pub use themed_icon_color_type::ThemedIconColorType;
pub use themed_icon_layer_type::LayerRole;
pub use themed_icon_layers::ThemedIconLayers;
pub use themed_icon_toggle_behaviors::ToggleBehaviors;
pub use themed_icon_types::ThemedIconTypes;
