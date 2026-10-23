//! Finds a byte pattern inside a loaded module.
//!
//! The module is found through the address of one of its functions
//! (NetHook2 uses its `CreateInterface` factory): the page info gives the
//! module's base address and the PE headers give its image size.

use std::ffi::c_void;
use std::mem::size_of;
use std::ptr;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

use windows::Win32::System::Memory::{VirtualQuery, MEMORY_BASIC_INFORMATION};

/// The `PE\0\0` signature of a Windows NT header.
const IMAGE_NT_SIGNATURE: u32 = 0x0000_4550;

/// The offset of `SizeOfImage` inside the optional header of a PE image.
const SIZE_OF_IMAGE_OFFSET: usize = 0x50;

/// The address of a function that lives inside the module that is being scanned.
static SIGSCAN_DLLFUNC: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

/// The base address of the module that is being scanned.
static BASE_ADDR: AtomicUsize = AtomicUsize::new(0);

/// The size of the image of the module that is being scanned.
static BASE_LEN: AtomicUsize = AtomicUsize::new(0);

/// Points the scanner at the module that owns `func`.
/// Points the scanner at the module that owns `func`.
pub fn set_dllfunc(func: *mut c_void) {
    SIGSCAN_DLLFUNC.store(func, Ordering::Release);
}

/// A byte pattern scanner.
#[derive(Debug)]
pub struct SigScan {
    /// The bytes to look for.
    sig_str: Vec<u8>,
    /// The mask that says which bytes of the pattern are significant.
    sig_mask: Vec<u8>,
    /// The length of the pattern.
    sig_len: usize,
    /// Whether the last scan succeeded.
    pub is_set: bool,
    /// The address the pattern was found at.
    pub sig_addr: *mut c_void,
}

impl SigScan {
    /// Creates a scanner that has not scanned anything yet.
    pub fn new() -> SigScan {
        SigScan {
            sig_str: Vec::new(),
            sig_mask: Vec::new(),
            sig_len: 0,
            is_set: false,
            sig_addr: ptr::null_mut(),
        }
    }

    /// Looks for `sig` using `mask` and remembers where it was found.
    ///
    /// Returns `0` on success, `1` when the pattern wasn't found and `2` when
    /// the module's memory info isn't available.
    pub fn init(&mut self, sig: &[u8], mask: &[u8]) -> i32 {
        self.is_set = false;

        // The mask length is the pattern length, not the signature length. A
        // shorter mask only compares the leading bytes; a longer one also
        // compares the bytes past the end of the signature (the NUL of the
        // string literal). Both are relied on.
        self.sig_len = mask.len();
        self.sig_str = sig.iter().copied().take(self.sig_len).collect();
        self.sig_str.resize(self.sig_len, 0);

        // A zero byte in the mask is not a wildcard, so it compares the byte exactly.
        self.sig_mask = mask.iter().copied().take(self.sig_len).collect();
        self.sig_mask.resize(self.sig_len, 0);

        if BASE_ADDR.load(Ordering::Acquire) == 0 {
            return 2;
        }

        self.sig_addr = self.find_signature();
        if self.sig_addr.is_null() {
            return 1;
        }

        self.is_set = true;
        0
    }

    /// Scans the module for the pattern that was set up by [`SigScan::init`].
    fn find_signature(&self) -> *mut c_void {
        let base = BASE_ADDR.load(Ordering::Acquire);
        let len = BASE_LEN.load(Ordering::Acquire);

        if base == 0 || len == 0 || self.sig_len == 0 {
            return ptr::null_mut();
        }

        // SAFETY: the module image is mapped for its whole lifetime, so the whole
        // `[base, base + len)` range is readable.
        let image = unsafe { std::slice::from_raw_parts(base as *const u8, len) };

        if self.sig_len > image.len() {
            return ptr::null_mut();
        }

        for window in image.windows(self.sig_len) {
            let mut matched = true;

            for (index, expected) in self.sig_str.iter().enumerate() {
                if self.sig_mask[index] != b'?' && *expected != window[index] {
                    matched = false;
                    break;
                }
            }

            if matched {
                return window.as_ptr() as *mut c_void;
            }
        }

        ptr::null_mut()
    }

    /// Reads the base address and the size of the module that holds the scanned function.
    pub fn get_dll_mem_info() -> bool {
        let func = SIGSCAN_DLLFUNC.load(Ordering::Acquire);

        BASE_ADDR.store(0, Ordering::Release);
        BASE_LEN.store(0, Ordering::Release);

        if func.is_null() {
            return false;
        }

        let mut info = std::mem::MaybeUninit::<MEMORY_BASIC_INFORMATION>::uninit();

        // SAFETY: `info` points at a buffer of exactly the size the API expects.
        let written = unsafe {
            VirtualQuery(
                Some(func as *const c_void),
                info.as_mut_ptr(),
                size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };

        if written == 0 {
            return false;
        }

        // SAFETY: the query succeeded, so the buffer was filled in.
        let info = unsafe { info.assume_init() };
        let base = info.AllocationBase as usize;

        if base == 0 {
            return false;
        }

        // The PE headers of the module: the DOS header holds the offset of the NT
        // headers, and the optional header holds the size of the image.
        //
        // SAFETY: the allocation base is the start of a mapped PE image.
        let (signature, size_of_image) = unsafe {
            let dos = base as *const u8;
            let e_lfanew = std::ptr::read_unaligned(dos.add(0x3C) as *const u32) as usize;

            if e_lfanew == 0 || e_lfanew > 0x1000 {
                return false;
            }

            let nt = base + e_lfanew;
            let signature = std::ptr::read_unaligned(nt as *const u32);
            let size_of_image =
                std::ptr::read_unaligned((nt + SIZE_OF_IMAGE_OFFSET) as *const u32) as usize;

            (signature, size_of_image)
        };

        if signature != IMAGE_NT_SIGNATURE || size_of_image == 0 {
            return false;
        }

        BASE_ADDR.store(base, Ordering::Release);
        BASE_LEN.store(size_of_image, Ordering::Release);

        true
    }
}

impl Default for SigScan {
    fn default() -> SigScan {
        SigScan::new()
    }
}
