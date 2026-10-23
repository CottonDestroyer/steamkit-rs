//! The network hooks.
//!
//! Two functions of the Steam client are hooked: the WebSocket frame builder,
//! which sees every message the client sends, and `CCMInterface::RecvPkt`,
//! which sees every message the server sends.
//!
//! The original hooks were `__fastcall`, which only exists on 32-bit Windows.
//! x86-64 has a single calling convention, so these use the system ABI.

use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::Ordering;
use std::sync::Mutex;

use crate::csimpledetour::{FnSlot, SimpleDetour};
use crate::csimplescan::SimpleScan;
use crate::logger::{logger, NetDirection};
use crate::steam::net::{e_web_socket_op_code_to_name, CNetPacket, EWebSocketOpCode};
use crate::steamclient::STEAMCLIENT_DLL;

/// The signature of `CWebSocketConnection::BBuildAndAsyncSendFrame`.
pub type BBuildAndAsyncSendFrameFn =
    unsafe extern "system" fn(*mut c_void, EWebSocketOpCode, *const u8, u32) -> bool;

/// The signature of `CCMInterface::RecvPkt`.
pub type RecvPktFn = unsafe extern "system" fn(*mut c_void, *mut CNetPacket);

/// The address of `CWebSocketConnection::BBuildAndAsyncSendFrame`, or of its
/// trampoline once the hook is installed.
pub static BUILD_AND_ASYNC_SEND_FRAME_ORIG: FnSlot = FnSlot::new(ptr::null_mut());

/// The address of `CCMInterface::RecvPkt`, or of its trampoline once the hook is
/// installed.
pub static RECV_PKT_ORIG: FnSlot = FnSlot::new(ptr::null_mut());

/// The global network hook.
static NET: Mutex<Option<Net>> = Mutex::new(None);

/// The byte pattern of `CWebSocketConnection::BBuildAndAsyncSendFrame`.
const BUILD_SIGNATURE: &[u8] = if cfg!(target_arch = "x86_64") {
    &[
        0x48, 0x8B, 0xC4, 0x55, 0x48, 0x8D, 0x68, 0x00, 0x48, 0x81, 0xEC, 0x00, 0x00, 0x00, 0x00,
        0x48, 0x89, 0x70, 0x00, 0x49, 0x8B, 0xF0, 0x48, 0x89, 0x78, 0x00, 0x4C, 0x89, 0x60,
    ]
} else {
    &[
        0x55, 0x8B, 0xEC, 0x83, 0xEC, 0xCC, 0xA1, 0xCC, 0xCC, 0xCC, 0xCC, 0x53, 0x8B, 0xD9, 0x89,
        0x5D, 0xCC, 0x83, 0x38, 0xCC, 0x75, 0xCC, 0x33, 0xC0, 0xEB, 0xCC, 0x8D, 0x45, 0xCC, 0x50,
        0x6A, 0xCC, 0x68, 0xCC, 0xCC, 0xCC, 0xCC, 0xFF, 0x15, 0xCC, 0xCC, 0xCC, 0xCC, 0x83, 0xC4,
        0xCC, 0x83, 0x7B, 0xCC, 0x02,
    ]
};

/// The mask of the byte pattern of `CWebSocketConnection::BBuildAndAsyncSendFrame`.
const BUILD_MASK: &[u8] = if cfg!(target_arch = "x86_64") {
    b"xxxxxxx?xxx????xxx?xxxxxx?xxx"
} else {
    b"xxxxx?x????xxxxx?xx?x?xxx?xx?xx?x????xx????xx?xx?x"
};

/// The byte pattern of `CCMInterface::RecvPkt`.
const RECV_SIGNATURE: &[u8] = if cfg!(target_arch = "x86_64") {
    &[
        0x48, 0x8B, 0xC4, 0x55, 0x48, 0x8D, 0xA8, 0xCC, 0xCC, 0xCC, 0xCC, 0x48, 0x81, 0xEC, 0xCC,
        0xCC, 0xCC, 0xCC, 0x48, 0x89, 0x58, 0x08, 0x48, 0x8B, 0xDA, 0x48, 0x89, 0x70, 0xCC, 0x48,
        0x8B, 0xF1,
    ]
} else {
    &[
        0x55, 0x8B, 0xEC, 0x81, 0xEC, 0xCC, 0xCC, 0xCC, 0xCC, 0xA1, 0xCC, 0xCC, 0xCC, 0xCC, 0x56,
        0x8B, 0xF1, 0x57,
    ]
};

/// The mask of the byte pattern of `CCMInterface::RecvPkt`.
const RECV_MASK: &[u8] = if cfg!(target_arch = "x86_64") {
    b"xxxxxxx????xxx????xxxxxxxxxx?xxx"
} else {
    b"xxxxx????x????xxxx"
};

/// The network hook of the Steam client.
#[derive(Debug)]
pub struct Net {
    recv_pkt_detour: Option<SimpleDetour>,
    build_detour: Option<SimpleDetour>,
}

impl Net {
    /// Finds the Steam client's networking routines and hooks them.
    pub fn new() -> Net {
        let mut net = Net {
            recv_pkt_detour: None,
            build_detour: None,
        };

        let mut scan = SimpleScan::with_dll(STEAMCLIENT_DLL);

        let build = scan.find_function(BUILD_SIGNATURE, BUILD_MASK);
        let build_address = build.unwrap_or(ptr::null_mut());
        BUILD_AND_ASYNC_SEND_FRAME_ORIG.store(build_address, Ordering::Release);

        logger().log_console(&format!(
            "CWebSocketConnection::BBuildAndAsyncSendFrame = {:p}\n",
            build_address
        ));

        let recv = scan.find_function(RECV_SIGNATURE, RECV_MASK);
        let recv_address = recv.unwrap_or(ptr::null_mut());
        RECV_PKT_ORIG.store(recv_address, Ordering::Release);

        logger().log_console(&format!("CCMInterface::RecvPkt = {:p}\n", recv_address));

        if build.is_some() {
            let replacement = b_build_and_async_send_frame as *mut c_void;
            let mut detour = SimpleDetour::new(&BUILD_AND_ASYNC_SEND_FRAME_ORIG, replacement);

            match detour.attach() {
                Ok(()) => {
                    logger().log_console("Detoured CWebSocketConnection::BBuildAndAsyncSendFrame!\n");
                    net.build_detour = Some(detour);
                }
                Err(error) => logger().log_console(&format!(
                    "Unable to hook CWebSocketConnection::BBuildAndAsyncSendFrame: {error:?}\n"
                )),
            }
        } else {
            logger().log_console(
                "Unable to hook CWebSocketConnection::BBuildAndAsyncSendFrame: func scan failed.\n",
            );
        }

        if recv.is_some() {
            let replacement = recv_pkt as *mut c_void;
            let mut detour = SimpleDetour::new(&RECV_PKT_ORIG, replacement);

            match detour.attach() {
                Ok(()) => {
                    logger().log_console("Detoured CCMInterface::RecvPkt!\n");
                    net.recv_pkt_detour = Some(detour);
                }
                Err(error) => logger()
                    .log_console(&format!("Unable to hook CCMInterface::RecvPkt: {error:?}\n")),
            }
        } else {
            logger().log_console("Unable to hook CCMInterface::RecvPkt: func scan failed.\n");
        }

        net
    }
}

impl Default for Net {
    fn default() -> Net {
        Net::new()
    }
}

/// Installs the global network hook.
pub fn init_net() {
    let net = Net::new();
    let mut guard = NET.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some(net);
}

/// Removes the global network hook, which detaches the hooks from the Steam client.
pub fn shutdown_net() {
    let mut guard = NET.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = None;
}

/// The hooked `CWebSocketConnection::BBuildAndAsyncSendFrame`.
pub unsafe extern "system" fn b_build_and_async_send_frame(
    web_socket_connection: *mut c_void,
    op_code: EWebSocketOpCode,
    data: *const u8,
    len: u32,
) -> bool {
    if op_code == EWebSocketOpCode::k_eWebSocketOpCode_Binary {
        let payload = if data.is_null() {
            &[][..]
        } else {
            std::slice::from_raw_parts(data, len as usize)
        };

        logger().log_net_message(NetDirection::Outgoing, payload);
    } else {
        logger().log_console(&format!(
            "Sending websocket frame with opcode {} ({}), ignoring\n",
            op_code as i32,
            e_web_socket_op_code_to_name(op_code)
        ));
    }

    let address = BUILD_AND_ASYNC_SEND_FRAME_ORIG.load(Ordering::Acquire);
    if address.is_null() {
        return false;
    }

    // SAFETY: the address holds the original function, either directly or through the
    // trampoline the detour installed.
    let original: BBuildAndAsyncSendFrameFn = std::mem::transmute(address);

    original(web_socket_connection, op_code, data, len)
}

/// The hooked `CCMInterface::RecvPkt`.
pub unsafe extern "system" fn recv_pkt(cm_connection: *mut c_void, packet: *mut CNetPacket) {
    if !packet.is_null() {
        let data = (*packet).m_pubData;
        let len = (*packet).m_cubData;

        let payload = if data.is_null() {
            &[][..]
        } else {
            std::slice::from_raw_parts(data, len as usize)
        };

        logger().log_net_message(NetDirection::Incoming, payload);
    }

    let address = RECV_PKT_ORIG.load(Ordering::Acquire);
    if address.is_null() {
        return;
    }

    // SAFETY: the address holds the original function, either directly or through the
    // trampoline the detour installed.
    let original: RecvPktFn = std::mem::transmute(address);

    original(cm_connection, packet);
}
