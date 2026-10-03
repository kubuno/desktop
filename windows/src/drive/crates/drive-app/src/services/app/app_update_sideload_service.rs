//! Port of `Files.App/Services/App/AppUpdateSideloadService.cs` (SIDELOAD channel).
//!
//! # What the original C# does
//!
//! Files distinguishes TWO update channels, selected at runtime based on
//! the package origin (see `AppLifecycleHelper.cs`):
//!
//! * **Store** — `StoreUpdateService.cs`. Relies on the WinRT
//!   `Windows.Services.Store`: `StoreContext.GetDefault()` then
//!   `GetAppAndOptionalStorePackageUpdatesAsync()` to list packages with updates,
//!   and `RequestDownloadAndInstallStorePackageUpdatesAsync(...)` to install.
//!   Reserved for the app packaged and distributed by the Microsoft Store: there is
//!   no URL or file, everything goes through the Store API. **Not applicable here**: the
//!   Rust port is not an MSIX package and has no `StoreContext`.
//!
//! * **Sideload** — `SideloadUpdateService.cs`. This is the channel reproduced here.
//!   The service downloads an `.appinstaller` file (XML) hosted on Files' CDN
//!   (and NOT on GitHub Releases, contrary to what one might
//!   think):
//!     - stable  : `https://cdn.files.community/files/stable/Files.Package.appinstaller`
//!     - preview : `https://cdn.files.community/files/preview/Files.Package.appinstaller`
//!
//!   The stable/preview choice depends on `Package.Current.Id.Name` ("Files" vs
//!   "FilesPreview"). The XML is deserialized (`XmlSerializer`) into an
//!   `AppInstaller` object:
//!     - `Version` attribute on the `<AppInstaller>` root — the remote version;
//!     - `<MainBundle Name=… Version=… Uri=… />` child element — the actual package.
//!
//!   The update is reported as available if, and only if:
//!   `MainBundle.Name == PackageName` AND `remoteVersion > PackageVersion`.
//!   Installation (`ApplyPackageUpdateAsync`) then hands the `.appinstaller` URI
//!   to `PackageManager.AddPackageByAppInstallerFileAsync(...)`,
//!   after `RegisterApplicationRestart` — so Windows reloads the bundle pointed to
//!   by the `.appinstaller` and restarts the app.
//!
//! # Port adaptation
//!
//! Since the port isn't packaged (no `PackageManager`, no `StoreContext`),
//! we faithfully reproduce the DETECTION PHASE of the sideload channel: an HTTP
//! request on the `.appinstaller`, extraction of the remote version and bundle URI,
//! comparison against the crate's current version (`CARGO_PKG_VERSION`). The
//! installation phase remains a documented TODO (see [`download_and_launch_update`]),
//! since it requires the Windows packaging subsystem, which is absent here.
//!
//! No HTTP crate exists in the workspace, so the request goes through
//! **WinHTTP** (`Win32::Networking::WinHttp`), without extra dependencies.

// NOT WIRED YET. Ported from Files' sideload update check; waits for an update UI and a Kubuno update feed.
// The allow is scoped to this file and must go once it is wired.
#![allow(dead_code)]

use windows::core::PCWSTR;
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable,
    WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE,
};

/// Stable channel URL — exact mirror of `SideloadUpdateService.SIDELOAD_STABLE`.
const SIDELOAD_STABLE_HOST: &str = "cdn.files.community";
const SIDELOAD_STABLE_PATH: &str = "/files/stable/Files.Package.appinstaller";
/// Full URL (for documentation): reconstructed in [`download_and_launch_update`].
const SIDELOAD_STABLE_URL: &str = "https://cdn.files.community/files/stable/Files.Package.appinstaller";

/// Current version of the port, used as `PackageVersion` for comparison.
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Short WinHTTP timeouts (ms): a network failure must not freeze startup.
const TIMEOUT_MS: i32 = 5_000;

/// Result of the check, exposed to the UI ("About" settings, notification).
///
/// Minimal mirror of the `IsUpdateAvailable` + version/URI fields that
/// `SideloadUpdateService` computes from the `.appinstaller`.
#[derive(Debug, Clone, Default)]
pub struct UpdateInfo {
    /// True if a strictly newer remote version was found.
    pub available: bool,
    /// Remote version read from the `AppInstaller/@Version` attribute (e.g. "4.0.20.0").
    pub version: Option<String>,
    /// URI of the bundle to install (`MainBundle/@Uri`), falling back to the `.appinstaller` URL.
    pub download_url: Option<String>,
}

/// Checks update availability via the SIDELOAD channel.
///
/// Reproduces `SideloadUpdateService.CheckForUpdatesAsync`: GET of the `.appinstaller`,
/// extraction of the remote version and bundle, comparison against [`CURRENT_VERSION`].
/// Any error (network, DNS, parsing) returns `UpdateInfo { available: false, .. }`
/// — exactly like the C#'s `catch (HttpRequestException)` which leaves
/// `IsUpdateAvailable = false`.
pub fn check_for_updates() -> UpdateInfo {
    // 1) Fetch the .appinstaller XML (stable channel, like the "Files" package).
    let xml = match http_get(SIDELOAD_STABLE_HOST, SIDELOAD_STABLE_PATH) {
        Some(body) if !body.is_empty() => body,
        _ => return UpdateInfo::default(),
    };

    // 2) Extract the remote version (Version attribute of the <AppInstaller> root)
    //    and the <MainBundle> URI. We stay lenient about the exact structure.
    let remote_version = match extract_appinstaller_version(&xml) {
        Some(v) => v,
        None => return UpdateInfo::default(),
    };
    let bundle_uri = extract_main_bundle_uri(&xml);

    // 3) Compare like the C#: available iff remoteVersion > PackageVersion.
    let available = is_newer(&remote_version, CURRENT_VERSION);

    UpdateInfo {
        available,
        version: Some(remote_version),
        // The C# installs via the .appinstaller URI; we prioritize exposing the
        // actual bundle URI when present, falling back to the .appinstaller URL.
        download_url: Some(bundle_uri.unwrap_or_else(|| SIDELOAD_STABLE_URL.to_string())),
    }
}

/// Downloads and launches the update installer.
///
/// **TODO** — not implemented. In the C# (`ApplyPackageUpdateAsync`), this step
/// hands the `.appinstaller` URI to
/// `PackageManager.AddPackageByAppInstallerFileAsync(uri,
/// AddPackageByAppInstallerOptions.ForceTargetAppShutdown, volume)` after
/// `PInvoke.RegisterApplicationRestart(...)`, which installs the bundle and
/// restarts the app. Since the port isn't packaged as MSIX, this will require either:
///   - downloading the `.msix`/`.msixbundle` pointed to by `url` then installing it via
///     `PackageManager` (requires a packaged context / package identity), or
///   - downloading a standalone installer and launching it (`ShellExecute`), then quitting.
///
/// To be wired up once the port's distribution strategy is settled.
pub fn download_and_launch_update(_url: &str) {
    // TODO: see doc above (bundle download + installer launch).
}

// ---------------------------------------------------------------------------
// Lightweight XML extraction (no XML dependency in the workspace)
// ---------------------------------------------------------------------------

/// Reads the `Version` attribute of the `<AppInstaller … Version="…">` root.
fn extract_appinstaller_version(xml: &str) -> Option<String> {
    // Target the Version attribute located in the opening <AppInstaller …> tag.
    let start = xml.find("<AppInstaller")?;
    let tag_end = xml[start..].find('>').map(|i| start + i)?;
    extract_attribute(&xml[start..tag_end], "Version")
}

/// Reads the `Uri` attribute of the `<MainBundle … Uri="…" />` element.
fn extract_main_bundle_uri(xml: &str) -> Option<String> {
    let start = xml.find("<MainBundle")?;
    let tag_end = xml[start..].find('>').map(|i| start + i)?;
    extract_attribute(&xml[start..tag_end], "Uri")
}

/// Extracts `name="value"` (or `name='value'`) from a tag fragment.
fn extract_attribute(tag: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=");
    let mut from = 0;
    while let Some(pos) = tag[from..].find(&needle) {
        let abs = from + pos;
        // Check that this is indeed the start of an attribute (preceded by whitespace or start).
        let ok_boundary = abs == 0
            || tag[..abs]
                .chars()
                .last()
                .map(|c| c.is_whitespace())
                .unwrap_or(true);
        let after = abs + needle.len();
        if ok_boundary {
            let rest = &tag[after..];
            let quote = rest.chars().next()?;
            if quote == '"' || quote == '\'' {
                let val = &rest[1..];
                if let Some(end) = val.find(quote) {
                    return Some(val[..end].to_string());
                }
            }
        }
        from = after;
    }
    None
}

/// `remote > current` on dotted "a.b.c.d" versions (mirror of
/// `remoteVersion.CompareTo(PackageVersion) > 0`). Missing components
/// default to 0; non-numeric segments are ignored.
fn is_newer(remote: &str, current: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.split(['.', '+', '-'])
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let r = parse(remote);
    let c = parse(current);
    let n = r.len().max(c.len());
    for i in 0..n {
        let rv = r.get(i).copied().unwrap_or(0);
        let cv = c.get(i).copied().unwrap_or(0);
        if rv != cv {
            return rv > cv;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// HTTP request via WinHTTP (no HTTP crate in the workspace)
// ---------------------------------------------------------------------------

/// Synchronous HTTPS GET of a small text resource via WinHTTP.
///
/// Returns the response body as a `String`, or `None` on failure (DNS,
/// connection, TLS, status not read…). Handles are closed cleanly.
fn http_get(host: &str, path: &str) -> Option<String> {
    // Convert strings to NUL-terminated UTF-16 for the PCWSTRs.
    let host_w = to_wide(host);
    let path_w = to_wide(path);
    let agent_w = to_wide("Drive/Update");
    let verb_w = to_wide("GET");

    unsafe {
        // WinHttpOpen — session. Returned handles are *mut c_void (NULL on failure).
        let session = WinHttpOpen(
            PCWSTR(agent_w.as_ptr()),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0u32,
        );
        if session.is_null() {
            return None;
        }

        // Short timeouts: resolution, connection, send, receive.
        let _ = WinHttpSetTimeouts(session, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS);

        // WinHttpConnect — to the host, port 443 (HTTPS).
        let connect = WinHttpConnect(session, PCWSTR(host_w.as_ptr()), 443, 0);
        if connect.is_null() {
            let _ = WinHttpCloseHandle(session);
            return None;
        }

        // WinHttpOpenRequest — GET <path>, HTTP/1.1 by default, secure channel.
        let request = WinHttpOpenRequest(
            connect,
            PCWSTR(verb_w.as_ptr()),
            PCWSTR(path_w.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null_mut(),
            WINHTTP_FLAG_SECURE,
        );
        if request.is_null() {
            let _ = WinHttpCloseHandle(connect);
            let _ = WinHttpCloseHandle(session);
            return None;
        }

        let body = (|| -> Option<String> {
            // Send the request (no body).
            WinHttpSendRequest(request, None, None, 0, 0, 0).ok()?;
            // Wait for the response.
            WinHttpReceiveResponse(request, std::ptr::null_mut()).ok()?;

            // Read in chunks: QueryDataAvailable then ReadData until 0.
            let mut buf: Vec<u8> = Vec::new();
            loop {
                let mut available: u32 = 0;
                WinHttpQueryDataAvailable(request, &mut available).ok()?;
                if available == 0 {
                    break;
                }
                let mut chunk = vec![0u8; available as usize];
                let mut read: u32 = 0;
                WinHttpReadData(
                    request,
                    chunk.as_mut_ptr() as *mut _,
                    available,
                    &mut read,
                )
                .ok()?;
                if read == 0 {
                    break;
                }
                chunk.truncate(read as usize);
                buf.extend_from_slice(&chunk);
                // Safety guard: the .appinstaller is tiny; cap at 1 MB.
                if buf.len() > 1_048_576 {
                    break;
                }
            }
            Some(String::from_utf8_lossy(&buf).into_owned())
        })();

        // Close handles unconditionally.
        let _ = WinHttpCloseHandle(request);
        let _ = WinHttpCloseHandle(connect);
        let _ = WinHttpCloseHandle(session);

        body
    }
}

/// Converts a `&str` to a NUL-terminated `Vec<u16>` for the WinHTTP APIs.
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_comparees_comme_le_csharp() {
        assert!(is_newer("4.0.20.0", "0.1.0"));
        assert!(is_newer("1.0.1", "1.0.0"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("0.9.9", "1.0.0"));
    }

    #[test]
    fn extraction_attributs_appinstaller() {
        let xml = r#"<?xml version="1.0"?>
<AppInstaller xmlns="http://schemas.microsoft.com/appx/appinstaller/2018" Uri="https://x" Version="4.0.20.0">
  <MainBundle Name="Files" Version="4.0.20.0" Uri="https://cdn.files.community/x.msixbundle" />
</AppInstaller>"#;
        assert_eq!(extract_appinstaller_version(xml).as_deref(), Some("4.0.20.0"));
        assert_eq!(
            extract_main_bundle_uri(xml).as_deref(),
            Some("https://cdn.files.community/x.msixbundle")
        );
    }
}
