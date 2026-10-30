//! Capture Named Pipe client identity via impersonation.

use std::io;

use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{
    CheckTokenMembership, CreateWellKnownSid, GetTokenInformation, RevertToSelf, TokenUser,
    WinBuiltinAdministratorsSid, WinLocalSystemSid, PSID, TOKEN_QUERY, TOKEN_USER,
    WELL_KNOWN_SID_TYPE,
};
use windows::Win32::System::Pipes::ImpersonateNamedPipeClient;
use windows::Win32::System::Threading::{GetCurrentThread, OpenThreadToken};

use crate::auth::{ClientIdentity, TransportKind};

/// Impersonates the Named Pipe client and reads SID / admin / LocalSystem flags.
///
/// `raw_handle` is the server-end pipe `RawHandle` as `isize`.
pub fn identity_from_named_pipe(raw_handle: isize) -> io::Result<ClientIdentity> {
    let pipe = HANDLE(raw_handle as *mut _);
    unsafe {
        ImpersonateNamedPipeClient(pipe)
            .map_err(|e| io::Error::new(io::ErrorKind::PermissionDenied, e.to_string()))?;

        let result = capture_impersonated_identity();
        let _ = RevertToSelf();
        result
    }
}

unsafe fn capture_impersonated_identity() -> io::Result<ClientIdentity> {
    let mut token = HANDLE::default();
    OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &mut token)
        .map_err(|e| io::Error::new(io::ErrorKind::PermissionDenied, e.to_string()))?;

    let identity = read_token(token);
    let _ = CloseHandle(token);
    identity
}

unsafe fn read_token(token: HANDLE) -> io::Result<ClientIdentity> {
    let mut needed = 0u32;
    let _ = GetTokenInformation(token, TokenUser, None, 0, &mut needed);
    if needed == 0 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "GetTokenInformation size failed",
        ));
    }
    let mut buf = vec![0u8; needed as usize];
    GetTokenInformation(
        token,
        TokenUser,
        Some(buf.as_mut_ptr().cast()),
        needed,
        &mut needed,
    )
    .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    let user = &*(buf.as_ptr() as *const TOKEN_USER);
    let sid = sid_to_string(user.User.Sid)?;
    let is_local_system = sid_is_well_known(user.User.Sid, WinLocalSystemSid);
    let is_admin = token_is_admin(token);

    Ok(ClientIdentity {
        transport: TransportKind::NamedPipe,
        sid: Some(sid),
        is_admin,
        is_local_system,
        unverified: false,
    })
}

unsafe fn sid_to_string(sid: PSID) -> io::Result<String> {
    let mut raw = PWSTR::null();
    ConvertSidToStringSidW(sid, &mut raw)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    if raw.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "ConvertSidToStringSidW returned null",
        ));
    }
    let s = raw.to_string().unwrap_or_default();
    let _ = LocalFree(Some(HLOCAL(raw.0 as *mut _)));
    Ok(s)
}

unsafe fn sid_is_well_known(sid: PSID, kind: WELL_KNOWN_SID_TYPE) -> bool {
    let mut size = 0u32;
    let _ = CreateWellKnownSid(kind, None, None, &mut size);
    if size == 0 {
        return false;
    }
    let mut buf = vec![0u8; size as usize];
    if CreateWellKnownSid(kind, None, Some(PSID(buf.as_mut_ptr().cast())), &mut size).is_err() {
        return false;
    }
    // Compare string forms to avoid EqualSid feature churn.
    let Ok(left) = sid_to_string(sid) else {
        return false;
    };
    let Ok(right) = sid_to_string(PSID(buf.as_mut_ptr().cast())) else {
        return false;
    };
    left.eq_ignore_ascii_case(&right)
}

unsafe fn token_is_admin(token: HANDLE) -> bool {
    let mut size = 0u32;
    let _ = CreateWellKnownSid(WinBuiltinAdministratorsSid, None, None, &mut size);
    if size == 0 {
        return false;
    }
    let mut buf = vec![0u8; size as usize];
    if CreateWellKnownSid(
        WinBuiltinAdministratorsSid,
        None,
        Some(PSID(buf.as_mut_ptr().cast())),
        &mut size,
    )
    .is_err()
    {
        return false;
    }
    let mut is_member = false.into();
    if CheckTokenMembership(Some(token), PSID(buf.as_mut_ptr().cast()), &mut is_member).is_err() {
        return false;
    }
    is_member.as_bool()
}
