//! The injector: the part of NetHook2 that runs outside the Steam process.
//! It's driven by `rundll32`:
//!
//! ```text
//! rundll32.exe C:\Path\To\NetHook2.dll,Inject
//! rundll32.exe C:\Path\To\NetHook2.dll,Inject <process ID>
//! rundll32.exe C:\Path\To\NetHook2.dll,Inject <process name>
//! rundll32.exe C:\Path\To\NetHook2.dll,Eject
//! ```
//!
//! `Inject` loads the DLL into the Steam process with a remote `LoadLibraryW` call,
//! `Eject` frees it again with a remote `FreeLibrary` call.

use std::ffi::{c_char, c_void, CStr};
use std::mem::size_of;

use windows::core::PCSTR;
use windows::Win32::Foundation::{CloseHandle, HINSTANCE, HWND, MAX_PATH};
use windows::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows::Win32::System::LibraryLoader::{
    GetModuleFileNameA, GetModuleFileNameW, GetModuleHandleA, GetProcAddress,
};
use windows::Win32::System::Memory::{
    VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE_READWRITE,
    PAGE_READWRITE,
};
use windows::Win32::System::ProcessStatus::{
    EnumProcessModules, EnumProcesses, GetModuleFileNameExA,
};
use windows::Win32::System::SystemInformation::IMAGE_FILE_MACHINE;
use windows::Win32::System::Threading::{
    CreateRemoteThread, GetCurrentProcess, IsWow64Process2, OpenProcess, WaitForSingleObject,
    LPTHREAD_START_ROUTINE, PROCESS_ALL_ACCESS, PROCESS_QUERY_INFORMATION,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
};
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxA, MB_ICONASTERISK, MB_OK};

use crate::nh2_string::{string_case_insensitive_ends_with, string_case_insensitive_eq};
use crate::sedebug::se_debug_acquire;

/// The name of the Steam client library, as it is looked for in a remote process.
const STEAMCLIENT_DLL_BYTES: &[u8] = if cfg!(target_arch = "x86_64") {
    b"steamclient64.dll"
} else {
    b"steamclient.dll"
};

/// The machine code that frees the DLL inside the target process.
///
/// A Rust function can't simply be copied into another process (it isn't
/// position independent and depends on the Rust runtime), so this is
/// hand-assembled.
///
/// It takes the address of the parameter block as its argument and calls
/// `GetModuleHandleW` and `FreeLibrary` through the pointers stored there.
const EJECT_CODE: &[u8] = if cfg!(target_arch = "x86_64") {
    &[
        0x53, // push rbx
        0x48, 0x83, 0xEC, 0x20, // sub rsp, 0x20
        0x48, 0x89, 0xCB, // mov rbx, rcx
        0x48, 0x89, 0xD9, // mov rcx, rbx
        0x48, 0x83, 0xC1, 0x10, // add rcx, 0x10
        0xFF, 0x53, 0x00, // call qword ptr [rbx]
        0x48, 0x89, 0xC1, // mov rcx, rax
        0xFF, 0x53, 0x08, // call qword ptr [rbx+8]
        0x48, 0x83, 0xC4, 0x20, // add rsp, 0x20
        0x5B, // pop rbx
        0x31, 0xC0, // xor eax, eax
        0xC3, // ret
    ]
} else {
    &[
        0x8B, 0x44, 0x24, 0x04, // mov eax, [esp+4]
        0x8D, 0x48, 0x08, // lea ecx, [eax+8]
        0x51, // push ecx
        0xFF, 0x10, // call dword ptr [eax]
        0x50, // push eax
        0x8B, 0x54, 0x24, 0x08, // mov edx, [esp+8]
        0x50, // push eax
        0xFF, 0x52, 0x04, // call dword ptr [edx+4]
        0x83, 0xC4, 0x04, // add esp, 4
        0x31, 0xC0, // xor eax, eax
        0xC2, 0x04, 0x00, // ret 4 (thread routines are stdcall)
    ]
};

/// The offset of the module name inside the parameter block of the ejector.
const fn eject_params_name_offset() -> usize {
    if cfg!(target_arch = "x86_64") {
        16
    } else {
        8
    }
}

/// The reason a Steam process could not be found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SteamProcessSearchError {
    /// Everything is fine.
    None,
    /// Steam is not running.
    CouldNotFindSteam,
    /// No process with the supplied name exists.
    CouldNotFindProcessWithSuppliedName,
    /// More than one process with the supplied name exists.
    FoundMultipleProcessesWithSuppliedName,
    /// The target process does not have the Steam client library loaded.
    TargetProcessDoesNotHaveSteamClientDllLoaded,
    /// The target process has a different architecture than NetHook2.
    TargetDifferentArchitecture,
}

/// Returns the message that describes a search failure.
fn name_from_steam_process_search_error(error: SteamProcessSearchError) -> &'static str {
    match error {
        SteamProcessSearchError::CouldNotFindSteam => {
            "Unable to find Steam. Make sure Steam is running, then try again."
        }
        SteamProcessSearchError::CouldNotFindProcessWithSuppliedName => {
            "Unable to find any processes with the supplied name."
        }
        SteamProcessSearchError::FoundMultipleProcessesWithSuppliedName => {
            "Multiple processes found with the supplied name. Please supply a process ID instead."
        }
        SteamProcessSearchError::TargetProcessDoesNotHaveSteamClientDllLoaded => {
            "Invalid process: Target process does not have steamclient.dll loaded."
        }
        SteamProcessSearchError::TargetDifferentArchitecture => {
            "Invalid process: Target process is not the same architecture as NetHook2."
        }
        SteamProcessSearchError::None => "Unknown error.",
    }
}

/// Shows a message box to the user.
fn message_box(hwindow: HWND, text: &str) {
    // SAFETY: both strings are NUL terminated.
    unsafe {
        let _ = MessageBoxA(
            Some(hwindow),
            PCSTR(text.as_ptr()),
            PCSTR(b"NetHook2\0".as_ptr()),
            MB_OK | MB_ICONASTERISK,
        );
    }
}

/// Injects NetHook2 into the Steam process.
///
/// This is the `Inject` export of the DLL; `rundll32` calls it with the window of the
/// caller, the handle of the DLL, the command line and the show command.
#[no_mangle]
#[allow(non_snake_case)]
pub extern "system" fn Inject(
    hwindow: HWND,
    hinstance: HINSTANCE,
    lpszcommandline: *mut c_char,
    _ncmdshow: i32,
) {
    let command_line = command_line_to_string(lpszcommandline);

    let mut debug_token = None;
    let mut steam_process_id: i32 = -1;
    let mut error = get_steam_process_id(&command_line, &mut steam_process_id);

    if needs_debug_privilege(error) {
        match unsafe { se_debug_acquire() } {
            Some(token) => debug_token = Some(token),
            None => {
                message_box(
                    hwindow,
                    "Unable to acquire SeDebug privilege. Make sure you're running as Administrator (elevated).",
                );
                return;
            }
        }

        error = get_steam_process_id(&command_line, &mut steam_process_id);
    }

    if error != SteamProcessSearchError::None {
        message_box(hwindow, name_from_steam_process_search_error(error));
        close_debug_token(debug_token);
        return;
    }

    let dll_path = own_module_path(hinstance);

    if process_has_module_loaded(steam_process_id, &dll_path, false) {
        message_box(
            hwindow,
            "Error: NetHook2 is already injected into this process.",
        );
        close_debug_token(debug_token);
        return;
    }

    let _ = self_inject_into_steam(hwindow, steam_process_id, &own_module_path_wide(hinstance));

    close_debug_token(debug_token);
}

/// Ejects NetHook2 from the Steam process.
///
/// This is the `Eject` export of the DLL.
#[no_mangle]
#[allow(non_snake_case)]
pub extern "system" fn Eject(
    hwindow: HWND,
    hinstance: HINSTANCE,
    lpszcommandline: *mut c_char,
    _ncmdshow: i32,
) {
    let command_line = command_line_to_string(lpszcommandline);

    let mut debug_token = None;
    let mut steam_process_id: i32 = -1;
    let mut error = get_steam_process_id(&command_line, &mut steam_process_id);

    if needs_debug_privilege(error) {
        match unsafe { se_debug_acquire() } {
            Some(token) => debug_token = Some(token),
            None => {
                message_box(
                    hwindow,
                    "Unable to acquire SeDebug privilege. Make sure you're running as Administrator (elevated).",
                );
                return;
            }
        }

        error = get_steam_process_id(&command_line, &mut steam_process_id);
    }

    if error != SteamProcessSearchError::None {
        message_box(hwindow, name_from_steam_process_search_error(error));
        close_debug_token(debug_token);
        return;
    }

    let dll_path = own_module_path(hinstance);

    if !process_has_module_loaded(steam_process_id, &dll_path, false) {
        message_box(
            hwindow,
            "Unable to eject NetHook2: This instance of Steam does not have NetHook2 loaded.",
        );
        close_debug_token(debug_token);
        return;
    }

    let _ = inject_ejection(hwindow, steam_process_id, &own_module_path_wide(hinstance));

    close_debug_token(debug_token);
}

/// Returns `true` when the search failed because the target process could not be opened.
fn needs_debug_privilege(error: SteamProcessSearchError) -> bool {
    matches!(
        error,
        SteamProcessSearchError::CouldNotFindSteam
            | SteamProcessSearchError::CouldNotFindProcessWithSuppliedName
            | SteamProcessSearchError::TargetProcessDoesNotHaveSteamClientDllLoaded
    )
}

/// Closes the debug token, if one was acquired.
fn close_debug_token(token: Option<windows::Win32::Foundation::HANDLE>) {
    if let Some(token) = token {
        // SAFETY: the token was opened by `se_debug_acquire`.
        unsafe { CloseHandle(token).ok() };
    }
}

/// Reads the command line `rundll32` passed to the export.
fn command_line_to_string(command_line: *mut c_char) -> String {
    if command_line.is_null() {
        return String::new();
    }

    // SAFETY: rundll32 passes a NUL terminated string.
    unsafe { CStr::from_ptr(command_line) }
        .to_string_lossy()
        .into_owned()
}

/// Returns the path of the NetHook2 DLL itself.
fn own_module_path(hinstance: HINSTANCE) -> Vec<u8> {
    let mut buffer = vec![0u8; MAX_PATH as usize];

    // SAFETY: the buffer is large enough for the path and the API terminates it.
    let length = unsafe {
        GetModuleFileNameA(Some(windows::Win32::Foundation::HMODULE(hinstance.0)), &mut buffer)
    } as usize;
    buffer.truncate(length);

    while buffer.last() == Some(&0) {
        buffer.pop();
    }

    buffer
}

/// Returns the path of the NetHook2 DLL itself, as UTF-16.
///
/// The path that is handed to the Steam process is loaded through `LoadLibraryW`, so
/// it has to survive characters that the ANSI code page cannot represent.
fn own_module_path_wide(hinstance: HINSTANCE) -> Vec<u16> {
    let mut buffer = vec![0u16; MAX_PATH as usize];

    // SAFETY: the buffer is large enough for the path and the API terminates it.
    let length = unsafe {
        GetModuleFileNameW(Some(windows::Win32::Foundation::HMODULE(hinstance.0)), &mut buffer)
    } as usize;
    buffer.truncate(length);

    while buffer.last() == Some(&0) {
        buffer.pop();
    }

    buffer
}

/// Determines the Steam process to inject into.
fn get_steam_process_id(
    command_line: &str,
    steam_process_id: &mut i32,
) -> SteamProcessSearchError {
    if command_line.is_empty() {
        *steam_process_id = find_steam_process_id();

        if *steam_process_id <= 0 {
            return SteamProcessSearchError::CouldNotFindSteam;
        }
    } else if !try_parse_int(command_line, steam_process_id) {
        let mut num_processes = 0;

        if !find_process_by_name(command_line, steam_process_id, &mut num_processes) {
            return SteamProcessSearchError::CouldNotFindProcessWithSuppliedName;
        }

        if num_processes > 1 {
            return SteamProcessSearchError::FoundMultipleProcessesWithSuppliedName;
        }
    }

    if process_is_different_architecture(*steam_process_id) {
        SteamProcessSearchError::TargetDifferentArchitecture
    } else if !process_has_module_loaded(
        *steam_process_id,
        STEAMCLIENT_DLL_BYTES,
        true,
    ) {
        SteamProcessSearchError::TargetProcessDoesNotHaveSteamClientDllLoaded
    } else {
        SteamProcessSearchError::None
    }
}

/// Returns the process id of the running Steam client.
fn find_steam_process_id() -> i32 {
    let mut process_id = 0;
    let mut num_processes = 0;

    if find_process_by_name("steam.exe", &mut process_id, &mut num_processes) && num_processes == 1 {
        return process_id;
    }

    -1
}

/// Looks for a process whose executable has the given name.
fn find_process_by_name(
    process_name: &str,
    first_process_id: &mut i32,
    num_processes: &mut i32,
) -> bool {
    const MAX_NUM_PROCESSES: usize = 2048;

    let mut processes = vec![0u32; MAX_NUM_PROCESSES];
    let mut needed = 0u32;

    // SAFETY: the buffer is large enough for the process ids the API reports.
    if unsafe {
        EnumProcesses(
            processes.as_mut_ptr(),
            (MAX_NUM_PROCESSES * size_of::<u32>()) as u32,
            &mut needed,
        )
    }
    .is_err()
    {
        *num_processes = 0;
        return false;
    }

    let enumerated = (needed as usize / size_of::<u32>()).min(MAX_NUM_PROCESSES);
    let mut found = 0;

    for process_id in processes[..enumerated].iter().copied() {
        // SAFETY: the process id comes from the operating system.
        let Ok(process) = (unsafe {
            OpenProcess(
                PROCESS_QUERY_INFORMATION | PROCESS_VM_READ,
                false,
                process_id,
            )
        }) else {
            continue;
        };

        let path = module_path_of(process, None);

        // SAFETY: the handle was opened by this function.
        unsafe { CloseHandle(process).ok() };

        if path.is_empty() {
            continue;
        }

        let mut ends_with_key = vec![b'\\'];
        ends_with_key.extend_from_slice(process_name.as_bytes());

        if string_case_insensitive_ends_with(&path, &ends_with_key) {
            if *first_process_id <= 0 {
                *first_process_id = process_id as i32;
            }

            found += 1;
        }
    }

    *num_processes = found;
    found > 0
}

/// Returns `true` when the process runs with a different architecture than NetHook2.
fn process_is_different_architecture(process_id: i32) -> bool {
    // SAFETY: the process id comes from the operating system.
    let Ok(process) =
        (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id as u32) })
    else {
        return false;
    };

    let mut process_machine = IMAGE_FILE_MACHINE(0);
    let mut own_machine = IMAGE_FILE_MACHINE(0);

    // SAFETY: both handles are valid for the duration of the call.
    let compared = unsafe {
        IsWow64Process2(process, &mut process_machine, None).is_ok()
            && IsWow64Process2(GetCurrentProcess(), &mut own_machine, None).is_ok()
    };

    // SAFETY: the handle was opened by this function.
    unsafe { CloseHandle(process).ok() };

    compared && process_machine != own_machine
}

/// Returns `true` when the process has the given module loaded.
fn process_has_module_loaded(
    process_id: i32,
    module_name: &[u8],
    partial_match_from_end: bool,
) -> bool {
    // SAFETY: the process id comes from the operating system.
    let Ok(process) = (unsafe {
        OpenProcess(
            PROCESS_QUERY_INFORMATION | PROCESS_VM_READ,
            false,
            process_id as u32,
        )
    }) else {
        return false;
    };

    let mut modules = vec![windows::Win32::Foundation::HMODULE::default(); 1024];
    let mut needed = 0u32;

    // SAFETY: the buffer is large enough for the module handles the API reports.
    let enumerated = unsafe {
        EnumProcessModules(
            process,
            modules.as_mut_ptr(),
            (modules.len() * size_of::<windows::Win32::Foundation::HMODULE>()) as u32,
            &mut needed,
        )
    }
    .is_ok()
    .then(|| (needed as usize / size_of::<windows::Win32::Foundation::HMODULE>()).min(modules.len()))
    .unwrap_or(0);

    let mut matched = false;

    for module in modules[..enumerated].iter().copied() {
        let path = module_path_of(process, Some(module));

        if path.is_empty() {
            continue;
        }

        let matches = if partial_match_from_end {
            string_case_insensitive_ends_with(&path, module_name)
        } else {
            string_case_insensitive_eq(&path, module_name)
        };

        if matches {
            matched = true;
            break;
        }
    }

    // SAFETY: the handle was opened by this function.
    unsafe { CloseHandle(process).ok() };

    matched
}

/// Returns the path of a module of a process, or of the process itself.
fn module_path_of(
    process: windows::Win32::Foundation::HANDLE,
    module: Option<windows::Win32::Foundation::HMODULE>,
) -> Vec<u8> {
    let mut buffer = vec![0u8; MAX_PATH as usize];

    // SAFETY: the buffer is large enough for the path and the API terminates it.
    let length = unsafe { GetModuleFileNameExA(Some(process), module, &mut buffer) } as usize;

    if length == 0 {
        return Vec::new();
    }

    buffer.truncate(length);

    while buffer.last() == Some(&0) {
        buffer.pop();
    }

    buffer
}

/// Parses a process id.
fn try_parse_int(text: &str, value: &mut i32) -> bool {
    if text.is_empty() {
        return false;
    }

    if !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }

    match text.parse::<i32>() {
        Ok(parsed) => {
            *value = parsed;
            true
        }
        Err(_) => false,
    }
}

/// Loads NetHook2 into the Steam process.
///
/// The path is passed as UTF-16 and loaded with `LoadLibraryW`, so install
/// paths outside the ANSI code page work.
fn self_inject_into_steam(
    hwindow: HWND,
    steam_process_id: i32,
    dll_path: &[u16],
) -> bool {
    // SAFETY: the process id comes from the operating system.
    let Ok(process) = (unsafe { OpenProcess(PROCESS_ALL_ACCESS, false, steam_process_id as u32) })
    else {
        message_box(hwindow, "Unable to open Steam process.");
        return false;
    };

    // SAFETY: the name is a NUL terminated literal.
    let Ok(kernel32) = (unsafe { GetModuleHandleA(PCSTR(b"kernel32.dll\0".as_ptr())) }) else {
        message_box(hwindow, "Unable to open load kernel32.dll.");
        unsafe { CloseHandle(process).ok() };
        return false;
    };

    // SAFETY: the name is a NUL terminated literal.
    let Some(load_library) =
        (unsafe { GetProcAddress(kernel32, PCSTR(b"LoadLibraryW\0".as_ptr())) })
    else {
        message_box(hwindow, "Unable to find LoadLibraryW.");
        unsafe { CloseHandle(process).ok() };
        return false;
    };

    // The argument of LoadLibraryW has to live in the address space of the target.
    let argument = unsafe {
        VirtualAllocEx(
            process,
            None,
            (dll_path.len() + 1) * 2,
            MEM_RESERVE | MEM_COMMIT,
            PAGE_READWRITE,
        )
    };

    if argument.is_null() {
        message_box(hwindow, "Unable to allocate memory inside Steam.");
        unsafe { CloseHandle(process).ok() };
        return false;
    }

    // The path has to be NUL terminated inside the target process.
    let mut argument_bytes = Vec::with_capacity((dll_path.len() + 1) * 2);
    for unit in dll_path {
        argument_bytes.extend_from_slice(&unit.to_le_bytes());
    }
    argument_bytes.extend_from_slice(&0u16.to_le_bytes());

    // SAFETY: the buffer holds exactly the bytes that are written.
    let written = unsafe {
        WriteProcessMemory(
            process,
            argument,
            argument_bytes.as_ptr() as *const c_void,
            argument_bytes.len(),
            None,
        )
    };

    if written.is_err() {
        message_box(hwindow, "Unable to write to allocated memory inside Steam.");
        unsafe {
            VirtualFreeEx(process, argument, 0, MEM_RELEASE).ok();
            CloseHandle(process).ok();
        }
        return false;
    }

    // SAFETY: the address is the `LoadLibraryW` export of kernel32.
    let start: LPTHREAD_START_ROUTINE = Some(unsafe {
        std::mem::transmute::<usize, unsafe extern "system" fn(*mut c_void) -> u32>(
            load_library as usize,
        )
    });

    // SAFETY: the process handle is valid and the thread only runs `LoadLibraryW`.
    let Ok(remote_thread) = (unsafe {
        CreateRemoteThread(
            process,
            None,
            0,
            start,
            Some(argument as *const c_void),
            0,
            None,
        )
    }) else {
        message_box(hwindow, "Unable to create remote thread inside Steam.");
        unsafe {
            VirtualFreeEx(process, argument, 0, MEM_RELEASE).ok();
            CloseHandle(process).ok();
        }
        return false;
    };

    // SAFETY: the thread handle was created by this function.
    let wait = unsafe { WaitForSingleObject(remote_thread, 10000) };

    unsafe {
        CloseHandle(remote_thread).ok();
        VirtualFreeEx(process, argument, 0, MEM_RELEASE).ok();
        CloseHandle(process).ok();
    }

    if wait == windows::Win32::Foundation::WAIT_TIMEOUT {
        message_box(hwindow, "Injection timed out.");
        return false;
    }

    true
}

/// Frees NetHook2 inside the Steam process.
///
/// The ejector is a small piece of machine code that gets copied into the
/// target process and started there. It calls the two function pointers from
/// the parameter block; the module name is passed as UTF-16 and looked up
/// with `GetModuleHandleW`.
fn inject_ejection(hwindow: HWND, steam_process_id: i32, module_name: &[u16]) -> bool {
    // SAFETY: the process id comes from the operating system.
    let Ok(process) = (unsafe { OpenProcess(PROCESS_ALL_ACCESS, false, steam_process_id as u32) })
    else {
        message_box(hwindow, "Unable to open Steam process.");
        return false;
    };

    // SAFETY: the name is a NUL terminated literal.
    let Ok(kernel32) = (unsafe { GetModuleHandleA(PCSTR(b"kernel32.dll\0".as_ptr())) }) else {
        message_box(hwindow, "Unable to open load kernel32.dll.");
        unsafe { CloseHandle(process).ok() };
        return false;
    };

    // SAFETY: the names are NUL terminated literals.
    let Some(get_module_handle) = (unsafe {
        GetProcAddress(kernel32, PCSTR(b"GetModuleHandleW\0".as_ptr()))
    }) else {
        message_box(hwindow, "Unable to find GetModuleHandleW.");
        unsafe { CloseHandle(process).ok() };
        return false;
    };

    // SAFETY: the name is a NUL terminated literal.
    let Some(free_library) =
        (unsafe { GetProcAddress(kernel32, PCSTR(b"FreeLibrary\0".as_ptr())) })
    else {
        message_box(hwindow, "Unable to find FreeLibrary.");
        unsafe { CloseHandle(process).ok() };
        return false;
    };

    // The parameter block holds the two function pointers and the name of the module.
    // The name has to stay two byte aligned for `GetModuleHandleW`, which the offsets
    // below guarantee.
    let pointer_size = size_of::<usize>();
    let name_offset = eject_params_name_offset();
    let mut parameters = vec![0u8; name_offset + (MAX_PATH as usize + 1) * 2];

    parameters[0..pointer_size]
        .copy_from_slice(&(get_module_handle as usize).to_le_bytes());
    parameters[pointer_size..pointer_size * 2]
        .copy_from_slice(&(free_library as usize).to_le_bytes());

    let name_length = module_name.len().min(MAX_PATH as usize);
    for (index, unit) in module_name[..name_length].iter().enumerate() {
        let offset = name_offset + index * 2;
        parameters[offset..offset + 2].copy_from_slice(&unit.to_le_bytes());
    }

    // SAFETY: the process handle is valid.
    let parameter_block = unsafe {
        VirtualAllocEx(
            process,
            None,
            parameters.len(),
            MEM_RESERVE | MEM_COMMIT,
            PAGE_READWRITE,
        )
    };

    if parameter_block.is_null() {
        message_box(hwindow, "Unable to allocate memory inside Steam.");
        unsafe { CloseHandle(process).ok() };
        return false;
    }

    // SAFETY: the buffer holds exactly the bytes that are written.
    let written = unsafe {
        WriteProcessMemory(
            process,
            parameter_block,
            parameters.as_ptr() as *const c_void,
            parameters.len(),
            None,
        )
    };

    if written.is_err() {
        message_box(hwindow, "Unable to write to allocated memory inside Steam.");
        unsafe {
            VirtualFreeEx(process, parameter_block, 0, MEM_RELEASE).ok();
            CloseHandle(process).ok();
        }
        return false;
    }

    // SAFETY: the process handle is valid.
    let code = unsafe {
        VirtualAllocEx(
            process,
            None,
            EJECT_CODE.len(),
            MEM_COMMIT,
            PAGE_EXECUTE_READWRITE,
        )
    };

    if code.is_null() {
        message_box(hwindow, "Unable to allocate executable memory inside Steam.");
        unsafe {
            VirtualFreeEx(process, parameter_block, 0, MEM_RELEASE).ok();
            CloseHandle(process).ok();
        }
        return false;
    }

    // SAFETY: the buffer holds exactly the bytes that are written.
    let written = unsafe {
        WriteProcessMemory(
            process,
            code,
            EJECT_CODE.as_ptr() as *const c_void,
            EJECT_CODE.len(),
            None,
        )
    };

    if written.is_err() {
        message_box(
            hwindow,
            "Unable to write to executable allocated memory inside Steam.",
        );
        unsafe {
            VirtualFreeEx(process, code, 0, MEM_RELEASE).ok();
            VirtualFreeEx(process, parameter_block, 0, MEM_RELEASE).ok();
            CloseHandle(process).ok();
        }
        return false;
    }

    // SAFETY: the address holds the machine code of the ejector.
    let start: LPTHREAD_START_ROUTINE = Some(unsafe {
        std::mem::transmute::<*mut c_void, unsafe extern "system" fn(*mut c_void) -> u32>(code)
    });

    // SAFETY: the process handle is valid and the thread only runs the ejector.
    let Ok(remote_thread) = (unsafe {
        CreateRemoteThread(
            process,
            None,
            0,
            start,
            Some(parameter_block as *const c_void),
            0,
            None,
        )
    }) else {
        message_box(hwindow, "Unable to create remote thread inside Steam.");
        unsafe {
            VirtualFreeEx(process, code, 0, MEM_RELEASE).ok();
            VirtualFreeEx(process, parameter_block, 0, MEM_RELEASE).ok();
            CloseHandle(process).ok();
        }
        return false;
    };

    // SAFETY: the thread handle was created by this function.
    let wait = unsafe { WaitForSingleObject(remote_thread, 5000) };

    unsafe {
        CloseHandle(remote_thread).ok();
        VirtualFreeEx(process, code, 0, MEM_RELEASE).ok();
        VirtualFreeEx(process, parameter_block, 0, MEM_RELEASE).ok();
        CloseHandle(process).ok();
    }

    if wait == windows::Win32::Foundation::WAIT_TIMEOUT {
        message_box(hwindow, "Injection timed out.");
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::{
        find_process_by_name, find_steam_process_id, get_steam_process_id, module_path_of,
        process_has_module_loaded, process_is_different_architecture, try_parse_int,
        SteamProcessSearchError,
    };
    use std::process;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
    };

    /// The id of the process the test runs in.
    fn current_process_id() -> i32 {
        process::id() as i32
    }

    /// Opens the current process with the rights the injector uses when it reads the
    /// module list of a target process.
    fn open_current_process() -> HANDLE {
        // SAFETY: opening the current process for querying always succeeds.
        unsafe {
            OpenProcess(
                PROCESS_QUERY_INFORMATION | PROCESS_VM_READ,
                false,
                process::id(),
            )
        }
        .expect("the current process can always be opened")
    }

    /// Closes a handle that a test opened.
    fn close(handle: HANDLE) {
        // SAFETY: the handle was opened by `open_current_process`.
        unsafe { CloseHandle(handle).ok() };
    }

    /// The file name of the running test binary, e.g. `steamkit_nethook-<hash>.exe`.
    fn current_exe_file_name() -> String {
        std::env::current_exe()
            .expect("the running test binary has a path")
            .file_name()
            .expect("the running test binary has a file name")
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn try_parse_int_accepts_a_plain_number() {
        let mut value = 0;
        assert!(try_parse_int("1234", &mut value));
        assert_eq!(value, 1234);

        let mut value = 0;
        assert!(try_parse_int("0", &mut value));
        assert_eq!(value, 0);

        // Leading zeros are accepted, like `atoi`.
        let mut value = 0;
        assert!(try_parse_int("007", &mut value));
        assert_eq!(value, 7);
    }

    #[test]
    fn try_parse_int_rejects_a_non_number() {
        for text in ["12a", "steam.exe", "-1", "+1", " 12", "12 ", "1 2"] {
            let mut value = 42;
            assert!(!try_parse_int(text, &mut value), "`{text}` is not a number");
            assert_eq!(value, 42, "a failed parse must not touch the output value");
        }
    }

    #[test]
    fn try_parse_int_rejects_the_empty_string() {
        let mut value = 42;
        assert!(!try_parse_int("", &mut value));
        assert_eq!(
            value, 42,
            "the empty string must not touch the output value"
        );
    }

    #[test]
    fn try_parse_int_rejects_a_number_that_does_not_fit_into_an_i32() {
        let mut value = 42;
        assert!(!try_parse_int("99999999999", &mut value));
        assert_eq!(value, 42);
    }

    #[test]
    fn find_process_by_name_reports_a_process_that_does_not_exist() {
        let mut first_process_id = 0;
        let mut num_processes = 0;

        assert!(!find_process_by_name(
            "steamkit-definitely-not-a-real-process",
            &mut first_process_id,
            &mut num_processes
        ));
        assert_eq!(num_processes, 0);
    }

    #[test]
    fn find_process_by_name_finds_the_running_test_binary() {
        let mut first_process_id = 0;
        let mut num_processes = 0;

        assert!(
            find_process_by_name(
                &current_exe_file_name(),
                &mut first_process_id,
                &mut num_processes
            ),
            "the enumerator must find the process it is running in"
        );
        assert_eq!(num_processes, 1);
        assert_eq!(first_process_id, current_process_id());
    }

    #[test]
    fn process_has_module_loaded_finds_a_module_that_is_loaded() {
        assert!(
            process_has_module_loaded(current_process_id(), b"kernel32.dll", true),
            "kernel32.dll is loaded into every process"
        );
    }

    #[test]
    fn process_has_module_loaded_rejects_a_module_that_is_not_loaded() {
        assert!(!process_has_module_loaded(
            current_process_id(),
            b"steamkit-not-a-real-module.dll",
            true
        ));
    }

    #[test]
    fn process_has_module_loaded_compares_the_whole_path_without_partial_matching() {
        let handle = open_current_process();
        let path = module_path_of(handle, None);
        close(handle);

        assert!(!path.is_empty());
        assert!(
            process_has_module_loaded(current_process_id(), &path, false),
            "the exact path of the loaded executable must match"
        );
        assert!(
            !process_has_module_loaded(current_process_id(), b"kernel32.dll", false),
            "without partial matching the whole path is compared, so a bare file name never matches"
        );
    }

    #[test]
    fn module_path_of_returns_the_path_of_the_current_executable() {
        let handle = open_current_process();
        let path = module_path_of(handle, None);
        close(handle);

        assert!(
            !path.is_empty(),
            "the current process has an executable path"
        );

        let exe = std::env::current_exe().expect("the running test binary has a path");
        let file_name = exe
            .file_name()
            .expect("the running test binary has a file name");

        assert!(
            path.ends_with(file_name.as_encoded_bytes()),
            "the module path must end in the file name of the current executable"
        );
        assert!(
            path.contains(&b'\\'),
            "the module path must be an absolute path"
        );
    }

    #[test]
    fn process_is_different_architecture_reports_the_same_architecture_for_the_current_process() {
        assert!(
            !process_is_different_architecture(current_process_id()),
            "the injector must accept a process of its own architecture"
        );
    }

    #[test]
    fn process_is_different_architecture_reports_the_same_architecture_for_a_process_it_cannot_open(
    ) {
        // `i32::MAX` cannot be a running process id, so the comparison cannot be made.
        assert!(!process_is_different_architecture(i32::MAX));
    }

    #[test]
    fn get_steam_process_id_reports_an_unknown_process_name() {
        let mut process_id = -1;

        assert_eq!(
            get_steam_process_id("steamkit-definitely-not-a-real-process", &mut process_id),
            SteamProcessSearchError::CouldNotFindProcessWithSuppliedName
        );
    }

    #[test]
    fn get_steam_process_id_reports_a_missing_steam_client_for_a_process_that_does_not_exist() {
        let mut process_id = -1;

        assert_eq!(
            get_steam_process_id("2147483647", &mut process_id),
            SteamProcessSearchError::TargetProcessDoesNotHaveSteamClientDllLoaded
        );
    }

    #[test]
    fn find_steam_process_id_agrees_with_the_process_enumerator() {
        let mut first_process_id = 0;
        let mut num_processes = 0;
        let steam_exists =
            find_process_by_name("steam.exe", &mut first_process_id, &mut num_processes);

        let steam_process_id = find_steam_process_id();

        if !steam_exists {
            assert_eq!(
                steam_process_id, -1,
                "without a running Steam no Steam process can be found"
            );
        } else if num_processes == 1 {
            assert_eq!(steam_process_id, first_process_id);
        } else {
            assert_eq!(
                steam_process_id, -1,
                "an ambiguous Steam must not be picked as the target"
            );
        }
    }
}
