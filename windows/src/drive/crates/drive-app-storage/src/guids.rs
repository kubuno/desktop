//! GUIDs that are missing from windows-rs metadata.
//! Port of `Files.App.CsWin32/ManualGuid.cs` (only the values this crate needs).

use windows::core::GUID;

/// Undocumented shell verb handler that pins an item to Quick Access ("frequent places").
/// `CLSID_PinToFrequentExecute` in `ManualGuid.cs`.
pub const CLSID_PIN_TO_FREQUENT_EXECUTE: GUID = GUID::from_u128(0xB455F46E_E4AF_4035_B0A4_CF18D2F6F28E);

/// Undocumented shell verb handler that unpins an item from Quick Access.
/// `CLSID_UnPinFromFrequentExecute` in `ManualGuid.cs`.
pub const CLSID_UNPIN_FROM_FREQUENT_EXECUTE: GUID = GUID::from_u128(0xEE20EEBA_DF64_4A4E_B7BB_2D1C6B2DFCC1);

/// `CLSID_NewMenu` — the shell "New" context-menu handler.
pub const CLSID_NEW_MENU: GUID = GUID::from_u128(0xD969A300_E7FF_11D0_A93B_00A0C90F2719);

/// Quick Access virtual folder (`Files.App.Storage` hardcodes this in `HomeFolder`).
pub const FOLDERID_QUICK_ACCESS: GUID = GUID::from_u128(0x3936E9E4_D92C_4EEE_A85A_BC16D5EA0819);

/// `FOLDERID_NetHood` — network shortcuts folder.
pub const FOLDERID_NETHOOD: GUID = GUID::from_u128(0xC5ABBF53_E17F_4121_8900_86626FC2C973);

/// `FOLDERID_Recent` — recent items folder.
pub const FOLDERID_RECENT: GUID = GUID::from_u128(0xAE50C081_EBD2_438A_8655_8A092E34987A);
