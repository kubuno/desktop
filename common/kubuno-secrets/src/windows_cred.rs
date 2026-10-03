//! Windows Credential Manager back-end: generic credentials named `Kubuno/<scope>/<item>`, persisted
//! `CRED_PERSIST_LOCAL_MACHINE` (DPAPI-protected for the current user, never roaming).

use windows_sys::Win32::Foundation::{GetLastError, ERROR_NOT_FOUND, FILETIME};
use windows_sys::Win32::Security::Credentials::{
    CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
};

use crate::{Secret, SecretError, SecretName};

const BACKEND: &str = "windows-credential-manager";
/// `CRED_MAX_CREDENTIAL_BLOB_SIZE` (5 * 512).
const MAX_BLOB: usize = 2560;

#[derive(Debug, Default)]
pub(crate) struct WindowsCredentialStore;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn last_error(what: &str, name: &SecretName) -> SecretError {
    // SAFETY: reads the calling thread's last-error value; no pointers involved.
    let code = unsafe { GetLastError() };
    SecretError::Backend { backend: BACKEND, message: format!("{what} {name} failed (Win32 error {code})") }
}

impl WindowsCredentialStore {
    pub(crate) fn backend(&self) -> &'static str {
        BACKEND
    }

    pub(crate) fn get(&self, name: &SecretName) -> Result<Option<Secret>, SecretError> {
        let target = wide(&name.target());
        let mut cred: *mut CREDENTIALW = std::ptr::null_mut();
        // SAFETY: `target` is a NUL-terminated UTF-16 string that outlives the call; `cred` receives a buffer
        // allocated by the API, released below with `CredFree`.
        let ok = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut cred) };
        if ok == 0 {
            // SAFETY: as above.
            let code = unsafe { GetLastError() };
            if code == ERROR_NOT_FOUND {
                return Ok(None);
            }
            return Err(SecretError::Backend { backend: BACKEND, message: format!("reading {name} failed (Win32 error {code})") });
        }
        if cred.is_null() {
            return Err(SecretError::Backend { backend: BACKEND, message: format!("reading {name} returned no credential") });
        }
        // SAFETY: `cred` is a valid CREDENTIALW returned by CredReadW; the blob pointer is valid for
        // `CredentialBlobSize` bytes until `CredFree`. The bytes are copied before freeing.
        let bytes = unsafe {
            let c = &*cred;
            let out = if c.CredentialBlob.is_null() || c.CredentialBlobSize == 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize).to_vec()
            };
            CredFree(cred.cast());
            out
        };
        Ok(Some(Secret::new(bytes)))
    }

    pub(crate) fn set(&self, name: &SecretName, value: &Secret) -> Result<(), SecretError> {
        let blob = value.expose();
        if blob.len() > MAX_BLOB {
            return Err(SecretError::TooLarge { name: name.target(), backend: BACKEND, len: blob.len(), max: MAX_BLOB });
        }
        let mut target = wide(&name.target());
        let mut user = wide("kubuno");
        let mut comment = wide("Kubuno desktop");
        let cred = CREDENTIALW {
            Flags: 0,
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_mut_ptr(),
            Comment: comment.as_mut_ptr(),
            LastWritten: FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 },
            // Checked against MAX_BLOB above, so it fits in a u32.
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_ptr().cast_mut(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            AttributeCount: 0,
            Attributes: std::ptr::null_mut(),
            TargetAlias: std::ptr::null_mut(),
            UserName: user.as_mut_ptr(),
        };
        // SAFETY: every pointer in `cred` points into a buffer that outlives the call; CredWriteW copies what it
        // keeps and does not write through the blob pointer.
        let ok = unsafe { CredWriteW(&cred, 0) };
        if ok == 0 {
            return Err(last_error("writing", name));
        }
        Ok(())
    }

    pub(crate) fn delete(&self, name: &SecretName) -> Result<bool, SecretError> {
        let target = wide(&name.target());
        // SAFETY: `target` is a NUL-terminated UTF-16 string that outlives the call.
        let ok = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
        if ok == 0 {
            // SAFETY: reads the thread's last-error value.
            let code = unsafe { GetLastError() };
            if code == ERROR_NOT_FOUND {
                return Ok(false);
            }
            return Err(SecretError::Backend { backend: BACKEND, message: format!("deleting {name} failed (Win32 error {code})") });
        }
        Ok(true)
    }
}
