//! `security_view_model` (mirrors `ViewModels/Properties/SecurityViewModel.cs`,
//! item `AccessControlEntry` = [`Ace`]) + `StorageSecurityService.GetAcl`.
//!
//! Reads the DACL and owner of the file object via
//! `GetNamedSecurityInfo(SE_FILE_OBJECT, OWNER|DACL)`, enumerates the ACEs
//! (`GetAclInformation` + `GetAce` loop), resolves each SID to a readable name
//! (`ConvertSidToStringSid` + `LookupAccountSid`), and decodes the access mask
//! of the selected ACE into permissions (Full control, Modify, Read & execute,
//! Read, Write, List folder contents for a folder), exactly like the
//! `AllowedXAccess`/`DeniedXAccess` properties of `AccessControlEntry`.
//!
//! READ ONLY: adding/removing ACEs (`AddAce`/`DeleteAce`), editable Deny
//! checkboxes, and the "Advanced Permissions" page are TODOs.

use std::ffi::c_void;

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{LocalFree, ERROR_ACCESS_DENIED, ERROR_SUCCESS, HLOCAL};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, GetNamedSecurityInfoW, SE_FILE_OBJECT,
};
use windows::Win32::Security::{
    GetAce, GetAclInformation, IsValidAcl, LookupAccountSidW, AclSizeInformation, ACE_HEADER,
    ACL, ACL_SIZE_INFORMATION, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PSID,
    PSECURITY_DESCRIPTOR, SID_NAME_USE, SidTypeAlias, SidTypeGroup, SidTypeUser,
    SidTypeWellKnownGroup,
};

use crate::views::properties::PropertiesTarget;

/// The "access allowed" ACE type (`ACCESS_ALLOWED_ACE_TYPE = 0`).
const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;

/// An access control entry (`AccessControlEntry`), resolved and decoded.
pub struct Ace {
    /// Readable name (`Principal.DisplayName`: account name, else the SID).
    pub display_name: String,
    /// Domain\name in parentheses (`FullNameHumanizedWithBrackes`), empty otherwise.
    pub full_name: String,
    /// Glyph for the principal type (user/group/unknown).
    pub glyph: &'static str,
    /// Allowed mask (Allow-type ACE) — 0 for a Deny ACE.
    pub allow_mask: u32,
    /// Denied mask (Deny-type ACE) — 0 for an Allow ACE. Gathered, but the tab
    /// does not show deny entries' rights yet.
    #[allow(dead_code)]
    pub deny_mask: u32,
}

/// The state of the Security tab (`SecurityViewModel`), gathered on open.
pub struct SecurityModel {
    /// Whether to show the elements (`DisplayElements`) or the error message.
    pub display: bool,
    /// Error message (`ErrorMessage`) when `!display`.
    pub error: String,
    /// Readable owner name (`AccessControlList.Owner`).
    pub owner: String,
    /// Whether the target is a folder (`AccessControlList.IsFolder`).
    pub is_folder: bool,
    /// The DACL's ACEs (`AccessControlEntries`).
    pub aces: Vec<Ace>,
}

impl SecurityModel {
    /// `GetAcl`: reads owner + DACL and decodes the ACEs. Works for a
    /// file, a folder, or a drive.
    pub fn gather(target: &PropertiesTarget) -> Self {
        let (path, is_folder) = match target {
            PropertiesTarget::Path(p) => (p.clone(), std::path::Path::new(p).is_dir()),
            PropertiesTarget::Drive(letter) => (format!("{letter}:\\"), true),
            PropertiesTarget::Multi(_) => {
                // Multi-selection: not handled (the ACL is specific to one object).
                return SecurityModel {
                    display: false,
                    error: kubuno_drive_desktop_localization::tr("SecurityUnableToDisplayPermissions").to_string(),
                    owner: String::new(),
                    is_folder: false,
                    aces: Vec::new(),
                };
            }
        };
        unsafe { read_acl(&path, is_folder) }
    }
}

/// Win32 read of the DACL + owner (mirrors `GetAcl` + `GetOwner`).
unsafe fn read_acl(path: &str, is_folder: bool) -> SecurityModel {
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();

    let mut psid_owner = PSID(std::ptr::null_mut());
    let mut pdacl: *mut ACL = std::ptr::null_mut();
    let mut psd = PSECURITY_DESCRIPTOR(std::ptr::null_mut());

    // OWNER + DACL in one pass (the original makes two calls, same result).
    let status = unsafe {
        GetNamedSecurityInfoW(
            PCWSTR(wide.as_ptr()),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            Some(&mut psid_owner),
            None,
            Some(&mut pdacl),
            None,
            &mut psd,
        )
    };

    if status != ERROR_SUCCESS {
        // Access denied → "read permissions required" message, generic otherwise.
        let msg = if status == ERROR_ACCESS_DENIED {
            format!(
                "{}\r\n{}",
                kubuno_drive_desktop_localization::tr("SecurityRequireReadPermissions"),
                kubuno_drive_desktop_localization::tr("SecurityClickAdvancedPermissions")
            )
        } else {
            kubuno_drive_desktop_localization::tr("SecurityUnableToDisplayPermissions").to_string()
        };
        if !psd.0.is_null() {
            unsafe { let _ = LocalFree(Some(HLOCAL(psd.0))); }
        }
        return SecurityModel {
            display: false,
            error: msg,
            owner: String::new(),
            is_folder,
            aces: Vec::new(),
        };
    }

    // Owner.
    let owner = unsafe { resolve_sid(psid_owner) }.map(|p| p.display).unwrap_or_default();

    let mut aces = Vec::new();
    let mut valid = false;

    if !pdacl.is_null() {
        valid = unsafe { IsValidAcl(pdacl) }.as_bool();
        let mut size_info = ACL_SIZE_INFORMATION::default();
        let ok = unsafe {
            GetAclInformation(
                pdacl,
                &mut size_info as *mut _ as *mut c_void,
                std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
                AclSizeInformation,
            )
        };
        if ok.is_ok() {
            for i in 0..size_info.AceCount {
                let mut pace: *mut c_void = std::ptr::null_mut();
                if unsafe { GetAce(pdacl, i, &mut pace) }.is_err() || pace.is_null() {
                    continue;
                }
                // ACE_HEADER then Mask (u32) then SidStart: layout of an
                // `ACCESS_ALLOWED_ACE`.
                let header = unsafe { *(pace as *const ACE_HEADER) };
                let mask = unsafe { *((pace as *const u8).add(std::mem::size_of::<ACE_HEADER>()) as *const u32) };
                // The SID starts after the header + mask (offset 8).
                let sid_ptr = unsafe {
                    (pace as *const u8).add(std::mem::size_of::<ACE_HEADER>() + std::mem::size_of::<u32>())
                };
                let principal = unsafe { resolve_sid(PSID(sid_ptr as *mut c_void)) };
                let (display_name, full_name, glyph) = match principal {
                    Some(p) => (p.display, p.full_name, p.glyph),
                    None => (String::new(), String::new(), "\u{E716}"),
                };
                // Allow if `AceType == ACCESS_ALLOWED_ACE_TYPE`, Deny otherwise.
                let (allow_mask, deny_mask) = if header.AceType == ACCESS_ALLOWED_ACE_TYPE {
                    (mask, 0)
                } else {
                    (0, mask)
                };
                aces.push(Ace { display_name, full_name, glyph, allow_mask, deny_mask });
            }
        }
    }

    if !psd.0.is_null() {
        unsafe { let _ = LocalFree(Some(HLOCAL(psd.0))); }
    }

    // `AccessControlList.IsValid`: the DACL is valid → show the elements.
    if valid {
        SecurityModel { display: true, error: String::new(), owner, is_folder, aces }
    } else {
        SecurityModel {
            display: false,
            error: kubuno_drive_desktop_localization::tr("SecurityUnableToDisplayPermissions").to_string(),
            owner,
            is_folder,
            aces,
        }
    }
}

struct ResolvedSid {
    display: String,
    full_name: String,
    glyph: &'static str,
}

/// `ConvertSidToStringSid` + `LookupAccountSid` (mirrors `AccessControlPrincipal`).
unsafe fn resolve_sid(sid: PSID) -> Option<ResolvedSid> {
    if sid.0.is_null() {
        return None;
    }
    // SID string (display fallback if the account can't be found).
    let sid_string = {
        let mut pstr = PWSTR::null();
        if unsafe { ConvertSidToStringSidW(sid, &mut pstr) }.is_ok() && !pstr.is_null() {
            let s = unsafe { pstr.to_string() }.unwrap_or_default();
            unsafe { let _ = LocalFree(Some(HLOCAL(pstr.0 as *mut c_void))); }
            s
        } else {
            String::new()
        }
    };

    // Size of the name/domain buffers (first call fails while setting the cch values).
    let mut cch_name = 0u32;
    let mut cch_domain = 0u32;
    let mut snu = SID_NAME_USE(0);
    let _ = unsafe {
        LookupAccountSidW(
            PCWSTR::null(),
            sid,
            None,
            &mut cch_name,
            None,
            &mut cch_domain,
            &mut snu,
        )
    };

    let (mut name, mut domain) = (String::new(), String::new());
    if cch_name > 0 {
        let mut name_buf = vec![0u16; cch_name as usize];
        let mut domain_buf = vec![0u16; cch_domain.max(1) as usize];
        let ok = unsafe {
            LookupAccountSidW(
                PCWSTR::null(),
                sid,
                Some(PWSTR(name_buf.as_mut_ptr())),
                &mut cch_name,
                Some(PWSTR(domain_buf.as_mut_ptr())),
                &mut cch_domain,
                &mut snu,
            )
        };
        if ok.is_ok() {
            name = String::from_utf16_lossy(&name_buf[..cch_name as usize]);
            domain = String::from_utf16_lossy(&domain_buf[..cch_domain as usize]);
        }
    }

    // Glyph based on the principal type (user / group / unknown).
    // Comparisons (the `windows` newtypes aren't `const` patterns).
    let glyph = if snu == SidTypeUser {
        "\u{E77B}"
    } else if snu == SidTypeAlias || snu == SidTypeGroup || snu == SidTypeWellKnownGroup {
        "\u{E902}"
    } else {
        "\u{E716}"
    };

    let display = if name.is_empty() { sid_string } else { name.clone() };
    let full_name = if domain.is_empty() || name.is_empty() {
        String::new()
    } else {
        format!("({domain}\\{name})")
    };
    Some(ResolvedSid { display, full_name, glyph })
}
