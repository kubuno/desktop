//! The file this library was loaded from.
//!
//! `kubuno-ui` ships as a Rust dylib, and a Rust dylib has no stable ABI. Every build of it is
//! therefore named after its link inputs, `kubuno_ui-<16 hex digits>.dll`, and every program
//! imports the exact name it was linked against (see the crate's `build.rs`): two builds coexist
//! in one folder, and a program whose build is missing fails in the loader with that name.
//!
//! Code that needs to know which build it runs on (the Visual Studio designer's `surfaceInfo`
//! handshake, diagnostics) asks [`module_path`] rather than guessing a file name.

use std::path::PathBuf;

/// The file stem every build of this library shares.
pub const FILE_STEM: &str = "kubuno_ui";

/// Whether `name` is one of this library's file names: `kubuno_ui-<16 hex digits>.dll`, or the
/// plain `kubuno_ui.dll` of a build made without the renaming link (case-insensitive).
pub fn is_library_file_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let Some(rest) = lower.strip_prefix(FILE_STEM).and_then(|r| r.strip_suffix(".dll")) else {
        return false;
    };
    rest.is_empty()
        || rest
            .strip_prefix('-')
            .is_some_and(|hash| hash.len() == 16 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The full path of the module holding this library's code: its `kubuno_ui-<hash>.dll`, or the
/// program itself when the library was linked statically (a unit-test harness). `None` when
/// Windows cannot say, and on other platforms.
pub fn module_path() -> Option<PathBuf> {
    imp::module_path()
}

#[cfg(windows)]
mod imp {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;

    const GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT: u32 = 0x2;
    const GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS: u32 = 0x4;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetModuleHandleExW(flags: u32, address: *const u16, module: *mut isize) -> i32;
        fn GetModuleFileNameW(module: isize, buffer: *mut u16, size: u32) -> u32;
    }

    /// A byte that lives in this library's own image: its address identifies the module. (A
    /// static is never duplicated into a caller - a program reaches it through its import.)
    static ANCHOR: u8 = 0;

    pub fn module_path() -> Option<PathBuf> {
        let mut module = 0isize;
        let address = std::ptr::addr_of!(ANCHOR).cast::<u16>();
        // SAFETY: `address` points into a loaded image; with UNCHANGED_REFCOUNT the handle is
        // borrowed, not owned, and it stays valid while this library is loaded.
        let found = unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                address,
                &mut module,
            )
        };
        if found == 0 || module == 0 {
            return None;
        }
        let mut buffer = vec![0u16; 32_768];
        let capacity = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
        // SAFETY: `buffer` holds `capacity` u16s; `module` is a live module handle.
        let len = unsafe { GetModuleFileNameW(module, buffer.as_mut_ptr(), capacity) } as usize;
        if len == 0 || len >= buffer.len() {
            return None;
        }
        Some(PathBuf::from(OsString::from_wide(&buffer[..len])))
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn module_path() -> Option<std::path::PathBuf> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_both_names() {
        assert!(is_library_file_name("kubuno_ui.dll"));
        assert!(is_library_file_name("kubuno_ui-0123456789abcdef.dll"));
        assert!(is_library_file_name("KUBUNO_UI-0123456789ABCDEF.DLL"));
        assert!(!is_library_file_name("kubuno_ui-0123.dll"));
        assert!(!is_library_file_name("kubuno_ui-0123456789abcdeg.dll"));
        assert!(!is_library_file_name("kubuno_ui.dll.lib"));
        assert!(!is_library_file_name("kubuno_views.dll"));
    }

    #[test]
    fn module_path_names_an_existing_file() {
        let path = module_path().expect("the module holding this test is known");
        assert!(path.is_file(), "{}", path.display());
    }
}
