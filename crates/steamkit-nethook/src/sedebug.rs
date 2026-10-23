//! `SeDebugPrivilege` handling. Steam protects its own process, so the
//! injector needs this privilege to open it and read its module list.

use std::ptr;

use windows::core::PCSTR;
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, HANDLE, LUID, ERROR_NO_TOKEN, ERROR_SUCCESS,
};
use windows::Win32::Security::{
    AdjustTokenPrivileges, ImpersonateSelf, LookupPrivilegeValueA, LUID_AND_ATTRIBUTES,
    SecurityImpersonation, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES,
    TOKEN_PRIVILEGES_ATTRIBUTES, TOKEN_QUERY, SE_PRIVILEGE_ENABLED,
};
use windows::Win32::System::Threading::{GetCurrentThread, OpenThreadToken};

/// The name of the privilege NetHook2 needs.
const SE_DEBUG_NAME: &[u8] = b"SeDebugPrivilege\0";

/// Enables or disables a privilege on `token`.
pub unsafe fn set_privilege(token: HANDLE, privilege: &[u8], enable: bool) -> bool {
    let mut luid = LUID::default();

    if LookupPrivilegeValueA(PCSTR::null(), PCSTR(privilege.as_ptr()), &mut luid).is_err() {
        return false;
    }

    // The first pass reads the current privilege state.
    let new_state = TOKEN_PRIVILEGES {
        PrivilegeCount: 1,
        Privileges: [LUID_AND_ATTRIBUTES {
            Luid: luid,
            Attributes: TOKEN_PRIVILEGES_ATTRIBUTES(0),
        }],
    };

    let mut previous_state = TOKEN_PRIVILEGES::default();
    let mut previous_length = std::mem::size_of::<TOKEN_PRIVILEGES>() as u32;

    let _ = AdjustTokenPrivileges(
        token,
        false,
        Some(&new_state),
        std::mem::size_of::<TOKEN_PRIVILEGES>() as u32,
        Some(&mut previous_state),
        Some(&mut previous_length),
    );

    if GetLastError() != ERROR_SUCCESS {
        return false;
    }

    // The second pass applies the requested change.
    previous_state.PrivilegeCount = 1;
    previous_state.Privileges[0].Luid = luid;

    let attributes = previous_state.Privileges[0].Attributes.0;
    previous_state.Privileges[0].Attributes = TOKEN_PRIVILEGES_ATTRIBUTES(if enable {
        attributes | SE_PRIVILEGE_ENABLED.0
    } else {
        attributes & !SE_PRIVILEGE_ENABLED.0
    });

    let _ = AdjustTokenPrivileges(
        token,
        false,
        Some(&previous_state),
        previous_length,
        None,
        None,
    );

    if GetLastError() != ERROR_SUCCESS {
        return false;
    }

    true
}

/// Acquires the `SeDebugPrivilege` for the current thread.
///
/// The returned handle has to be closed by the caller.
pub unsafe fn se_debug_acquire() -> Option<HANDLE> {
    let mut token = HANDLE(ptr::null_mut());

    if OpenThreadToken(
        GetCurrentThread(),
        TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
        false,
        &mut token,
    )
    .is_err()
    {
        if GetLastError() != ERROR_NO_TOKEN {
            return None;
        }

        if ImpersonateSelf(SecurityImpersonation).is_err() {
            return None;
        }

        if OpenThreadToken(
            GetCurrentThread(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            false,
            &mut token,
        )
        .is_err()
        {
            return None;
        }
    }

    if !set_privilege(token, SE_DEBUG_NAME, true) {
        let _ = CloseHandle(token);
        return None;
    }

    Some(token)
}
