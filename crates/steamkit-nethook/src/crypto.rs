//! The crypto hook.
//!
//! NetHook2 hooks Steam's `CCrypto::SymmetricEncryptChosenIV` to capture the
//! plaintext of every message the client sends, and it looks up Steam's
//! `PchMsgNameFromEMsg` to give the captured messages a readable name.

use std::ffi::{c_char, c_void};
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::Mutex;

use crate::csimpledetour::{FnSlot, SimpleDetour};
use crate::csimplescan::SimpleScan;
use crate::logger::{logger, NetDirection};
use crate::steam::emsg::EMsg;
use crate::steam::steamtypes::EServerType;
use crate::steamclient::STEAMCLIENT_DLL;

/// The signature of `CCrypto::SymmetricEncryptChosenIV`.
pub type SymmetricEncryptChosenIVFn = unsafe extern "C" fn(
    *const u8,
    u32,
    *const u8,
    u32,
    *mut u8,
    *mut u32,
    *const u8,
    u32,
) -> bool;

/// The signature of Steam's `PchMsgNameFromEMsg`.
pub type PchMsgNameFromEMsgFn = unsafe extern "C" fn(i32) -> *const c_char;

/// The address of `CCrypto::SymmetricEncryptChosenIV`, or of its trampoline once the
/// hook is installed.
pub static ENCRYPT_ORIG: FnSlot = FnSlot::new(ptr::null_mut());

/// The address of Steam's `PchMsgNameFromEMsg`.
static PCH_MSG_NAME_FROM_EMSG: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

/// The global crypto hook.
static CRYPTO: Mutex<Option<Crypto>> = Mutex::new(None);

/// The byte pattern of `CCrypto::SymmetricEncryptChosenIV`.
const ENCRYPT_SIGNATURE: &[u8] = if cfg!(target_arch = "x86_64") {
    &[
        0x48, 0x83, 0xEC, 0x58, 0x8B, 0x84, 0x24, 0xCC, 0xCC, 0xCC, 0xCC, 0xC6, 0x44, 0x24,
    ]
} else {
    &[
        0x55, 0x8B, 0xEC, 0x6A, 0xCC, 0xFF, 0x75, 0xCC, 0xFF, 0x75, 0xCC, 0xFF, 0x75, 0xCC, 0xFF,
        0x75, 0xCC, 0xFF, 0x75, 0xCC, 0xFF, 0x75, 0xCC, 0xFF, 0x75, 0xCC, 0xFF, 0x75, 0xCC, 0xFF,
        0x75,
    ]
};

/// The mask of the byte pattern of `CCrypto::SymmetricEncryptChosenIV`.
const ENCRYPT_MASK: &[u8] = if cfg!(target_arch = "x86_64") {
    b"xxxxxxx????xxx"
} else {
    b"xxxx?xx?xx?xx?xx?xx?xx?xx?xx"
};

/// The byte pattern of Steam's `PchMsgNameFromEMsg`.
const MSG_NAME_SIGNATURE: &[u8] = if cfg!(target_arch = "x86_64") {
    &[0x48, 0x89, 0x5C, 0x24, 0xCC, 0x57, 0x48, 0x83, 0xEC, 0x20, 0x8B, 0xD9, 0xE8]
} else {
    &[
        0x55, 0x8B, 0xEC, 0x51, 0x56, 0xE8, 0x00, 0x00, 0x00, 0x00, 0x8B, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x8B, 0xF0,
    ]
};

/// The mask of the byte pattern of Steam's `PchMsgNameFromEMsg`.
const MSG_NAME_MASK: &[u8] = if cfg!(target_arch = "x86_64") {
    b"xxxx?xxxxxxxx"
} else {
    b"xxxxxx????x?????xx"
};

/// The description of a Steam message.
///
/// Only kept to document the layout; the field types match Steam's struct.
#[derive(Clone, Copy, Debug)]
#[allow(non_snake_case)]
#[repr(C)]
pub struct MsgInfo {
    /// The message type.
    pub eMsg: EMsg,
    /// The message flags.
    pub nFlags: i32,
    /// The server the message is addressed to.
    pub k_EServerTarget: EServerType,
    /// Unknown.
    pub nUnk1: u32,
    /// The name of the message.
    pub pchMsgName: *const c_char,
}

/// The crypto hook of the Steam client.
#[derive(Debug)]
pub struct Crypto {
    encrypt_detour: Option<SimpleDetour>,
}

impl Crypto {
    /// Finds Steam's crypto routine and hooks it.
    pub fn new() -> Crypto {
        let mut crypto = Crypto {
            encrypt_detour: None,
        };

        let mut scan = SimpleScan::with_dll(STEAMCLIENT_DLL);

        let encrypt = scan.find_function(ENCRYPT_SIGNATURE, ENCRYPT_MASK);
        let encrypt_address = encrypt.unwrap_or(ptr::null_mut());
        ENCRYPT_ORIG.store(encrypt_address, Ordering::Release);

        logger().log_console(&format!(
            "CCrypto::SymmetricEncryptChosenIV = {:p}\n",
            encrypt_address
        ));

        match scan.find_function(MSG_NAME_SIGNATURE, MSG_NAME_MASK) {
            Some(address) => {
                PCH_MSG_NAME_FROM_EMSG.store(address, Ordering::Release);
                logger().log_console(&format!("PchMsgNameFromEMsg = {:p}\n", address));
            }
            None => logger().log_console("Unable to find PchMsgNameFromEMsg.\n"),
        }

        if encrypt.is_some() {
            let replacement = symmetric_encrypt_chosen_iv as *mut c_void;
            let mut detour = SimpleDetour::new(&ENCRYPT_ORIG, replacement);

            match detour.attach() {
                Ok(()) => {
                    logger().log_console("Detoured SymmetricEncryptChosenIV!\n");
                    crypto.encrypt_detour = Some(detour);
                }
                Err(error) => logger().log_console(&format!(
                    "Unable to hook SymmetricEncryptChosenIV: {error:?}\n"
                )),
            }
        } else {
            logger()
                .log_console("Unable to hook SymmetricEncryptChosenIV: Func scan failed.\n");
        }

        crypto
    }
}

impl Default for Crypto {
    fn default() -> Crypto {
        Crypto::new()
    }
}

/// Installs the global crypto hook.
pub fn init_crypto() {
    let crypto = Crypto::new();
    let mut guard = CRYPTO.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some(crypto);
}

/// Removes the global crypto hook, which detaches the hook from the Steam client.
pub fn shutdown_crypto() {
    let mut guard = CRYPTO.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = None;
}

/// Returns the name Steam gives to a message type.
///
/// Returns `None` if Steam's lookup function wasn't found; the logger prints
/// that as "(null)".
pub fn message_name(emsg: EMsg, _server_type: u8) -> Option<&'static str> {
    let address = PCH_MSG_NAME_FROM_EMSG.load(Ordering::Acquire);

    if address.is_null() {
        return None;
    }

    // SAFETY: the address was resolved from Steam's own export table.
    let lookup: PchMsgNameFromEMsgFn = unsafe { std::mem::transmute(address) };
    let name = unsafe { lookup(emsg.raw() as i32) };

    if name.is_null() {
        return None;
    }

    // SAFETY: the pointer refers to a NUL terminated string owned by the Steam client.
    unsafe { std::ffi::CStr::from_ptr(name) }.to_str().ok()
}

/// The hooked `CCrypto::SymmetricEncryptChosenIV`.
pub unsafe extern "C" fn symmetric_encrypt_chosen_iv(
    plaintext_data: *const u8,
    plaintext_len: u32,
    iv: *const u8,
    iv_len: u32,
    encrypted_data: *mut u8,
    encrypted_len: *mut u32,
    key: *const u8,
    key_len: u32,
) -> bool {
    let plaintext = if plaintext_data.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(plaintext_data, plaintext_len as usize)
    };

    logger().log_net_message(NetDirection::Outgoing, plaintext);

    let address = ENCRYPT_ORIG.load(Ordering::Acquire);
    if address.is_null() {
        return false;
    }

    // SAFETY: the address holds the original function, either directly or through the
    // trampoline the detour installed.
    let original: SymmetricEncryptChosenIVFn = std::mem::transmute(address);

    original(
        plaintext_data,
        plaintext_len,
        iv,
        iv_len,
        encrypted_data,
        encrypted_len,
        key,
        key_len,
    )
}
