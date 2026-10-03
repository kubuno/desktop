//! Typed, scoped, versioned settings (`docs/STORAGE-COMPONENTS.md` §4.1): the values ([`value`]), what an app
//! declares ([`schema`]) and the engine ([`store`]).

pub mod migrate;
pub mod schema;
pub mod serde_bridge;
pub mod store;
pub mod value;

pub use schema::{check_limits, looks_like_secret, valid_set_name, valid_setting_name, Layer, SettingDef, SettingScope, SettingsSchema, MAX_LIST_ITEMS, MAX_NAME, MAX_SET_BYTES, MAX_VALUE_BYTES};
pub use store::{make_backend, register_schema, registered_schema, registered_schema_of, set_and_save, ChangeOrigin, SettingChange, Settings, SettingsOptions, Subscription, Upgrade, UpgradeFn};
pub use value::{FromSetting, SettingType, SettingValue};
