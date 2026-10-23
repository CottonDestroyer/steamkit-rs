//! Finds functions in a loaded module by byte pattern.

use std::ffi::{c_void, CString};
use std::ptr;

use windows::core::PCSTR;
use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

use crate::sigscan::{set_dllfunc, SigScan};

/// The name of Valve's interface factory function.
const CREATE_INTERFACE_PROCNAME: &str = "CreateInterface";

/// Finds functions inside a loaded module by byte pattern.
#[derive(Debug)]
pub struct SimpleScan {
    interface_set: bool,
    interface: *mut c_void,
    signature: SigScan,
}

impl SimpleScan {
    /// Creates a scanner that is not bound to a module yet.
    pub fn new() -> SimpleScan {
        SimpleScan {
            interface_set: false,
            interface: ptr::null_mut(),
            signature: SigScan::new(),
        }
    }

    /// Creates a scanner bound to `filename`.
    pub fn with_dll(filename: &str) -> SimpleScan {
        let mut scan = SimpleScan::new();
        scan.set_dll(filename);
        scan
    }

    /// Binds the scanner to the module `filename`.
    pub fn set_dll(&mut self, filename: &str) -> bool {
        self.interface = sys_get_factory(filename);
        set_dllfunc(self.interface);

        if !SigScan::get_dll_mem_info() {
            self.interface_set = false;
        } else {
            self.interface_set = !self.interface.is_null();
        }

        self.interface_set
    }

    /// Looks for a function with the given byte pattern inside the scanned module.
    pub fn find_function(&mut self, sig: &[u8], mask: &[u8]) -> Option<*mut c_void> {
        if !self.interface_set {
            return None;
        }

        if self.signature.init(sig, mask) != 0 {
            return None;
        }

        if !self.signature.is_set {
            return None;
        }

        Some(self.signature.sig_addr)
    }
}

impl Default for SimpleScan {
    fn default() -> SimpleScan {
        SimpleScan::new()
    }
}

/// Returns the `CreateInterface` factory of the module `module_name`.
fn sys_get_factory(module_name: &str) -> *mut c_void {
    sys_get_proc_address(module_name, CREATE_INTERFACE_PROCNAME).unwrap_or(ptr::null_mut())
}

/// Returns the address of `proc_name` inside the module `module_name`.
fn sys_get_proc_address(module_name: &str, proc_name: &str) -> Option<*mut c_void> {
    let module_name = CString::new(module_name).ok()?;
    let proc_name = CString::new(proc_name).ok()?;

    // SAFETY: both strings are NUL terminated.
    let module = unsafe { GetModuleHandleA(PCSTR(module_name.as_ptr() as *const u8)) }.ok()?;
    let proc = unsafe { GetProcAddress(module, PCSTR(proc_name.as_ptr() as *const u8)) };

    proc.map(|function| function as usize as *mut c_void)
}
