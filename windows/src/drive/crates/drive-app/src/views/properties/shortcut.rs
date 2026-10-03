//! "Shortcut" tab of the properties sheet — mirror of
//! `Views/Properties/ShortcutPage.xaml(.cs)` driven by the "shortcut" part
//! of `ViewModels/Properties/Items/FileProperties.GetBaseProperties`.
//!
//! The original only shows this tab for an `IShortcutItem` (a `.lnk` or a
//! `.url`) and presents: the shortcut TYPE (`ShortcutItemType`: Application
//! / Link / File), the TARGET (`ShortcutItemPath` = `IShortcutItem.TargetPath`),
//! the ARGUMENTS (`ShortcutItemArguments`) and the "START IN" folder
//! (`ShortcutItemWorkingDir`), plus a clickable "Open file location" card
//! (`ShortcutItemOpenLinkCommand`). For a web link (`.url`,
//! `Item.IsLinkItem`) the Arguments / Start in rows are hidden
//! (`ShortcutItemArgumentsVisibility` / `ShortcutItemWorkingDirVisibility` =
//! false), as in the XAML.
//!
//! The port reads the fields directly from the shortcut (IShellLinkW for a
//! `.lnk`, the `[InternetShortcut]` INI for a `.url`) where the original
//! relies on `IWindowsShortcutService`. READ-ONLY: writing
//! (`SaveChangesAsync` → `UIFilesystemHelpers.UpdateShortcutItemProperties`)
//! and the "Run" combo (`ShowWindowCommand`) / "Run as administrator" switch
//! (`RunAsAdmin`) remain TODOs.

use crate::styles::theme::Theme;
use crate::ui::{Painter, Rect};

use super::PropertiesTarget;

/// Resolves the target of a `.lnk` (IShellLinkW) or `.url` (INI) shortcut.
/// Exposed so that `mod.rs` can decide whether the Compatibility tab applies
/// (target = an executable), like the factory that tests
/// `IShortcutItem.TargetPath`.
pub fn shortcut_target(path: &str) -> Option<String> {
    let ext = extension(path);
    match ext.as_str() {
        "lnk" => crate::utils::storage::resolve_shortcut(path),
        "url" => read_url_target(path),
        _ => None,
    }
}

/// `[InternetShortcut] URL=…` of a `.url` file (ANSI/UTF-8 INI format).
fn read_url_target(path: &str) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed
            .strip_prefix("URL=")
            .or_else(|| trimmed.strip_prefix("url="))
        {
            let v = rest.trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// The Shortcut tab's data, gathered on opening (the counterpart of
/// `FileProperties`'s `ViewModel.ShortcutItem* = …` assignments).
pub struct ShortcutModel {
    /// Could the shortcut be read (otherwise empty page).
    pub loaded: bool,
    /// Localized type (`ShortcutItemType`): "Application", "Link" or "File".
    pub shortcut_type: String,
    /// Path of the target (`ShortcutItemPath` = `TargetPath`).
    pub target_path: String,
    /// Command-line arguments (`ShortcutItemArguments`).
    pub arguments: String,
    /// Working folder (`ShortcutItemWorkingDir`).
    pub working_dir: String,
    /// Web link (`.url`, `Item.IsLinkItem`): hides Arguments / Start in.
    pub is_link_item: bool,
}

impl ShortcutModel {
    /// Gathers the fields for a shortcut. `None`/empty page if the target
    /// isn't a shortcut or is unreadable.
    pub fn gather(target: &PropertiesTarget) -> Self {
        let empty = || ShortcutModel {
            loaded: false,
            shortcut_type: String::new(),
            target_path: String::new(),
            arguments: String::new(),
            working_dir: String::new(),
            is_link_item: false,
        };
        let path = match target {
            PropertiesTarget::Path(p) => p.clone(),
            _ => return empty(),
        };
        let ext = extension(&path);
        let is_link_item = ext == "url";

        // Reading the fields depending on the shortcut type.
        let (target_path, arguments, working_dir) = if ext == "lnk" {
            read_lnk_fields(&path)
        } else if is_link_item {
            (read_url_target(&path).unwrap_or_default(), String::new(), String::new())
        } else {
            return empty();
        };

        // `ShortcutItemType` (cf. FileProperties): Application if the target
        // is an executable/msi, otherwise "Link" for a `.url`, otherwise "File".
        let is_application = is_executable(&target_path) || is_msi(&target_path);
        let shortcut_type = if is_application {
            drive_localization::tr("Application").to_string()
        } else if is_link_item {
            drive_localization::tr("PropertiesShortcutTypeLink").to_string()
        } else {
            drive_localization::tr("File").to_string()
        };

        ShortcutModel {
            loaded: true,
            shortcut_type,
            target_path,
            arguments,
            working_dir,
            is_link_item,
        }
    }
}

/// Reads (target, arguments, working folder) from a `.lnk` via IShellLinkW —
/// same mechanics as `utils::storage::resolve_shortcut`, in a single load.
fn read_lnk_fields(path: &str) -> (String, String, String) {
    use windows::core::{Interface, HSTRING};
    use windows::Win32::System::Com::{
        CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER, STGM_READ,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink, SLGP_RAWPATH};

    unsafe {
        let Ok(link) = CoCreateInstance::<_, IShellLinkW>(&ShellLink, None, CLSCTX_INPROC_SERVER)
        else {
            return (String::new(), String::new(), String::new());
        };
        let Ok(persist) = link.cast::<IPersistFile>() else {
            return (String::new(), String::new(), String::new());
        };
        if persist.Load(&HSTRING::from(path), STGM_READ).is_err() {
            return (String::new(), String::new(), String::new());
        }
        let read = |f: &dyn Fn(&mut [u16]) -> windows::core::Result<()>| -> String {
            let mut buf = [0u16; 1024];
            if f(&mut buf).is_ok() {
                let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                String::from_utf16_lossy(&buf[..len])
            } else {
                String::new()
            }
        };
        let target = {
            let mut buf = [0u16; 1024];
            if link
                .GetPath(&mut buf, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32)
                .is_ok()
            {
                let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                String::from_utf16_lossy(&buf[..len])
            } else {
                String::new()
            }
        };
        let arguments = read(&|b| link.GetArguments(b));
        let working_dir = read(&|b| link.GetWorkingDirectory(b));
        (target, arguments, working_dir)
    }
}

fn extension(path: &str) -> String {
    std::path::Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// `FileExtensionHelpers.IsExecutableFile(x, exeOnly:false)`.
fn is_executable(path: &str) -> bool {
    matches!(extension(path).as_str(), "exe" | "bat" | "cmd" | "ahk")
}

/// `FileExtensionHelpers.IsMsiFile`.
fn is_msi(path: &str) -> bool {
    extension(path) == "msi"
}

/// Draws the tab in `content`, offset by `scroll`. Returns
/// `(total height, rect of the "Open location" button)`. Clipped by the
/// caller. Mirror of the page's `StackPanel Padding=12 Spacing=4`.
pub fn draw(
    p: &Painter,
    theme: &Theme,
    content: &Rect,
    model: &ShortcutModel,
    scroll: f32,
) -> (f32, Option<Rect>) {
    let f = &p.renderer.formats;
    let pad = 12.0;
    let left = content.left + pad;
    let right = content.right - pad;
    let top0 = content.top + pad - scroll;
    let mut y = top0;

    if !model.loaded {
        return (0.0, None);
    }

    let lw = 140.0;

    // ---- Type + Target card (always shown) --------------------------------
    {
        let mut rows: Vec<(&str, &str)> = vec![
            (drive_localization::tr("PropertiesShortcutItemType.Text"), model.shortcut_type.as_str()),
            (drive_localization::tr("PropertiesShortcutItemPath.Text"), model.target_path.as_str()),
        ];
        y = draw_card(p, theme, left, right, y, lw, &mut rows) + 4.0;
    }

    // ---- Arguments + Start in card (hidden for a web link) ----------------
    if !model.is_link_item {
        let mut rows: Vec<(&str, &str)> = vec![
            (drive_localization::tr("PropertiesShortcutItemArgs.Text"), model.arguments.as_str()),
            (drive_localization::tr("ShortcutItemWorkingDir"), model.working_dir.as_str()),
        ];
        y = draw_card(p, theme, left, right, y, lw, &mut rows) + 4.0;
    }

    // ---- Clickable "Open file location" card -------------------------------
    // (`SettingsCard IsClickEnabled` + `ShortcutItemOpenLinkCommand`).
    let card = Rect::new(left, y, right, y + 56.0);
    p.fill_rounded(&card, 4.0, &theme.card_background);
    p.stroke_rounded(&card, 4.0, &theme.card_stroke);
    p.text(
        drive_localization::tr("OpenFileLocation"),
        &Rect::new(card.left + 16.0, card.top, card.right - 48.0, card.bottom),
        &f.body,
        &theme.text_primary,
        false,
    );
    // Action glyph (E8A7 "OpenInNewWindow") on the right.
    p.text(
        "\u{E8A7}",
        &Rect::new(card.right - 40.0, card.top, card.right - 12.0, card.bottom),
        &f.icon_small,
        &theme.text_secondary,
        true,
    );
    y = card.bottom;

    // TODO (writing): `UpdateShortcutItemProperties` on Save + "Run" combo
    // (`ShowWindowCommand`) + "Run as administrator" switch (`RunAsAdmin`).
    (y - top0 + pad, Some(card))
}

/// Card with Name(lw)/Value rows (the page's bordered `Grid`). `rows` carries
/// the (label, value) pairs.
fn draw_card(
    p: &Painter,
    theme: &Theme,
    left: f32,
    right: f32,
    top: f32,
    lw: f32,
    rows: &mut [(&str, &str)],
) -> f32 {
    let f = &p.renderer.formats;
    let row_h = 28.0;
    let card_h = 12.0 + rows.len() as f32 * row_h + 12.0;
    let card = Rect::new(left, top, right, top + card_h);
    p.fill_rounded(&card, 4.0, &theme.card_background);
    p.stroke_rounded(&card, 4.0, &theme.card_stroke);

    let ix = card.left + 12.0;
    let iright = card.right - 12.0;
    let mut ry = card.top + 12.0;
    for (name, value) in rows.iter() {
        let row = Rect::new(ix, ry, iright, ry + row_h);
        p.text(
            name,
            &Rect::new(row.left, row.top, row.left + lw, row.bottom),
            &f.body_strong,
            &theme.text_secondary,
            false,
        );
        // The value lives in a `TextBox` (editing = TODO): paint its frame.
        let vbox = Rect::new(row.left + lw + 8.0, row.top + 2.0, row.right, row.bottom - 2.0);
        p.fill_rounded(&vbox, 4.0, &theme.toolbar_background);
        p.stroke_rounded(&vbox, 4.0, &theme.card_stroke);
        p.text_ellipsis(
            value,
            &Rect::new(vbox.left + 8.0, vbox.top, vbox.right - 8.0, vbox.bottom),
            &f.body,
            &theme.text_primary,
        );
        ry = row.bottom;
    }
    card.bottom
}
