//! `compatibility_view_model` (mirrors
//! `ViewModels/Properties/CompatibilityViewModel.cs`) +
//! `Services/Windows/WindowsCompatibilityService.cs`.
//!
//! Reads the registry value
//! `HKCU\SOFTWARE\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers`
//! named by the executable's PATH, then decomposes it into options
//! (`WindowsCompatibilityOptions.FromString`): compatibility mode (Vista/7/8),
//! reduced color mode, 640×480, disable fullscreen optimizations, run as
//! administrator, restart, and DPI options.
//!
//! READ ONLY: writing (`SetCompatibilityOptionsForPath`) goes through an
//! ELEVATED PowerShell command in the original (`New-ItemProperty` /
//! `Remove-ItemProperty` on `HKCU:\…\Layers`) — not ported in this slice.

use crate::views::properties::PropertiesTarget;

/// The counterpart to `WindowsCompatibilityOptions`: the fields decoded from
/// the registry string. Each enum is stored by its already-localized display
/// label (no need to keep the variant for a read-only render).
pub struct CompatibilityModel {
    /// Whether the tab has a valid target (always true if it's shown).
    pub loaded: bool,
    /// Compatibility mode (`CompatibilityMode`).
    pub compat_mode: String,
    /// Reduced color mode (`ReducedColorMode`).
    pub reduced_color: String,
    /// Run in 640×480 (`RunIn40x480Resolution`).
    pub run_640: bool,
    /// Disable fullscreen optimizations (`DisableFullscreenOptimization`).
    pub disable_fullscreen: bool,
    /// Run as administrator (`RunAsAdministrator`).
    pub run_as_admin: bool,
    /// Register for restart (`RegisterForRestart`).
    pub register_restart: bool,
    /// High DPI setting (`HighDpiOption`).
    pub high_dpi_option: String,
    /// High DPI behavior override (`HighDpiOverride`).
    pub high_dpi_override: String,
}

impl CompatibilityModel {
    /// `GetCompatibilityOptionsForPath`: reads the registry value named by
    /// the path (the target's for a shortcut), then `FromString`.
    pub fn gather(target: &PropertiesTarget) -> Self {
        let item_path = match target {
            PropertiesTarget::Path(p) => {
                // `ItemPath = item is IShortcutItem sht ? sht.TargetPath : item.ItemPath`.
                crate::views::properties::shortcut::shortcut_target(p).unwrap_or_else(|| p.clone())
            }
            _ => String::new(),
        };
        let raw = read_layers_value(&item_path);
        from_string(raw.as_deref())
    }
}

/// Reads the registry `Layers` value (`HKCU`) named `path`. `None` if absent.
fn read_layers_value(path: &str) -> Option<String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, REG_VALUE_TYPE,
    };

    let sub =
        HSTRING::from("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\AppCompatFlags\\Layers");
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, &sub, None, KEY_READ, &mut hkey).is_err() {
            return None;
        }
        let name = HSTRING::from(path);
        let mut ty = REG_VALUE_TYPE::default();
        let mut size: u32 = 0;
        // First call: size of the data (in bytes, including terminator).
        let ok = RegQueryValueExW(
            hkey,
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut ty),
            None,
            Some(&mut size),
        )
        .is_ok();
        if !ok || size == 0 {
            let _ = RegCloseKey(hkey);
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        let ok = RegQueryValueExW(
            hkey,
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut ty),
            Some(buf.as_mut_ptr()),
            Some(&mut size),
        )
        .is_ok();
        let _ = RegCloseKey(hkey);
        if !ok {
            return None;
        }
        // REG_SZ: NUL-terminated UTF-16LE.
        let u16buf: Vec<u16> = buf
            .as_chunks::<2>().0.iter()
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let len = u16buf.iter().position(|&c| c == 0).unwrap_or(u16buf.len());
        Some(String::from_utf16_lossy(&u16buf[..len]))
    }
}

/// `WindowsCompatibilityOptions.FromString`: splits the string into tokens
/// (space-separated, leading "~" ignored) and rebuilds the options.
fn from_string(raw: Option<&str>) -> CompatibilityModel {
    // Descriptions (registry tokens) → variant, cf. `WindowsCompatibilityModes.cs`.
    let mut compat_mode = "";
    let mut reduced_color = "";
    let mut high_dpi_option = "";
    let mut dpi_override_flags: u32 = 0; // [Flags]: HIGHDPIAWARE=1, DPIUNAWARE=2, GDIDPISCALING=4.
    let mut run_640 = false;
    let mut disable_fullscreen = false;
    let mut run_as_admin = false;
    let mut register_restart = false;

    if let Some(raw) = raw {
        for token in raw.split_whitespace() {
            match token {
                // CompatibilityMode
                "VISTARTM" | "VISTASP1" | "VISTASP2" | "WIN7RTM" | "WIN8RTM" => compat_mode = token,
                // ReducedColorMode
                "256COLOR" | "16BITCOLOR" => reduced_color = token,
                // HighDpiOption
                "PERPROCESSSYSTEMDPIFORCEOFF" | "PERPROCESSSYSTEMDPIFORCEON" => high_dpi_option = token,
                // HighDpiOverride ([Flags], cumulative)
                "HIGHDPIAWARE" => dpi_override_flags |= 1,
                "DPIUNAWARE" => dpi_override_flags |= 2,
                "GDIDPISCALING" => dpi_override_flags |= 4,
                // Booleans
                "640X480" => run_640 = true,
                "DISABLEDXMAXIMIZEDWINDOWEDMODE" => disable_fullscreen = true,
                "RUNASADMIN" => run_as_admin = true,
                "REGISTERAPPRESTART" => register_restart = true,
                _ => {}
            }
        }
    }

    CompatibilityModel {
        loaded: true,
        compat_mode: compat_mode_label(compat_mode),
        reduced_color: reduced_color_label(reduced_color),
        run_640,
        disable_fullscreen,
        run_as_admin,
        register_restart,
        high_dpi_option: high_dpi_option_label(high_dpi_option),
        high_dpi_override: high_dpi_override_label(dpi_override_flags),
    }
}

/// Labels — mirrors the `Dictionary<…, string>` of `CompatibilityViewModel`.
fn compat_mode_label(desc: &str) -> String {
    match desc {
        // These values are LITERAL strings in the original (not localized).
        "VISTARTM" => "Windows Vista".into(),
        "VISTASP1" => "Windows Vista (Service Pack 1)".into(),
        "VISTASP2" => "Windows Vista (Service Pack 2)".into(),
        "WIN7RTM" => "Windows 7".into(),
        "WIN8RTM" => "Windows 8".into(),
        _ => drive_localization::tr("None").into(),
    }
}

fn reduced_color_label(desc: &str) -> String {
    match desc {
        "256COLOR" => drive_localization::tr("CompatibilityReducedColorModeColor8bit").into(),
        "16BITCOLOR" => drive_localization::tr("CompatibilityReducedColorModeColor16bit").into(),
        _ => drive_localization::tr("CompatibilityNoReducedColor").into(),
    }
}

fn high_dpi_option_label(desc: &str) -> String {
    match desc {
        "PERPROCESSSYSTEMDPIFORCEOFF" => drive_localization::tr("CompatibilityOnWindowsLogin").into(),
        "PERPROCESSSYSTEMDPIFORCEON" => drive_localization::tr("CompatibilityOnProgramStart").into(),
        _ => drive_localization::tr("CompatibilityDoNotAdjustDPI").into(),
    }
}

fn high_dpi_override_label(flags: u32) -> String {
    // Cf. `WindowsCompatDpiOverrideKind`: None=0, Application=1, System=2,
    // Advanced=4, SystemAdvanced=6 (GDIDPISCALING|DPIUNAWARE).
    match flags {
        1 => drive_localization::tr("Application").into(),
        2 => drive_localization::tr("System").into(),
        4 => drive_localization::tr("Advanced").into(),
        6 => drive_localization::tr("CompatibilitySystemEnhanced").into(),
        _ => drive_localization::tr("CompatibilityDoNotOverrideDPI").into(),
    }
}
