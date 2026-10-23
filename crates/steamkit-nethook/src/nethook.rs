//! The DLL entry point. When loaded into the Steam process it allocates a
//! console, makes sure the Steam client library is loaded, and installs the
//! crypto and network hooks.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::core::PCSTR;
use windows::Win32::Foundation::HINSTANCE;
use windows::Win32::System::Console::{AllocConsole, FreeConsole};
use windows::Win32::System::LibraryLoader::{GetModuleFileNameA, LoadLibraryA};
use windows::Win32::System::SystemServices::{DLL_PROCESS_ATTACH, DLL_PROCESS_DETACH};

use crate::crypto::{init_crypto, shutdown_crypto};
use crate::logger::{init_logger, logger};
use crate::net::{init_net, shutdown_net};
use crate::nh2_string::string_case_insensitive_ends_with;
use crate::steamclient::STEAMCLIENT_DLL;
use crate::version;

/// Whether the DLL allocated the console it writes to.
static OWNS_CONSOLE: AtomicBool = AtomicBool::new(false);

/// Returns `true` when the current process is `rundll32.exe`.
///
/// The injector half of NetHook2 runs inside `rundll32.exe`; the hooks must not be
/// installed there.
fn is_run_dll32() -> bool {
    let mut buffer = vec![0u8; windows::Win32::Foundation::MAX_PATH as usize];

    // SAFETY: the buffer is large enough for the path and the API terminates it.
    let length = unsafe { GetModuleFileNameA(None, &mut buffer) } as usize;
    buffer.truncate(length);

    while buffer.last() == Some(&0) {
        buffer.pop();
    }

    string_case_insensitive_ends_with(&buffer, b"\\rundll32.exe")
}

/// Writes the build information to the console.
fn print_version_info() {
    logger().log_console("Initializing NetHook2...\n");

    if version::BUILT_FROM_COMMIT_SHA.is_empty() {
        logger().log_console(&format!(
            "Built at {}. No further build information available.\n",
            version::BUILD_DATE
        ));
    } else {
        logger().log_console(&format!(
            "Built at {} from {}",
            version::BUILD_DATE,
            version::BUILT_FROM_COMMIT_SHA
        ));

        if version::built_from_dirty() {
            logger().log_console("/dirty");
        }

        logger().log_console(&format!(" ({})\n", version::BUILT_FROM_COMMIT_DATE));
    }
}

/// The entry point of the DLL.
#[no_mangle]
#[allow(non_snake_case)]
pub extern "system" fn DllMain(
    _hinst_dll: HINSTANCE,
    fdw_reason: u32,
    _lpv_reserved: *mut c_void,
) -> i32 {
    if is_run_dll32() {
        return 1;
    }

    match fdw_reason {
        DLL_PROCESS_ATTACH => {
            // SAFETY: allocating a console is always allowed.
            let owns_console = unsafe { AllocConsole() }.is_ok();
            OWNS_CONSOLE.store(owns_console, Ordering::Relaxed);

            // The signature scans need the Steam client library to be loaded.
            // SAFETY: the name is a NUL terminated string.
            unsafe {
                let _ = LoadLibraryA(PCSTR(STEAMCLIENT_DLL.as_ptr()));
            };

            init_logger();
            print_version_info();

            init_crypto();
            init_net();
        }
        DLL_PROCESS_DETACH => {
            shutdown_net();
            shutdown_crypto();

            if OWNS_CONSOLE.load(Ordering::Relaxed) {
                // SAFETY: the console was allocated by this DLL.
                unsafe { FreeConsole().ok() };
            }
        }
        _ => {}
    }

    1
}
