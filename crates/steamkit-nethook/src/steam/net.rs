//! Steam network types.

/// A handle to a Steam network connection.
pub type HCONNECTION = u32;

/// The opcode of a WebSocket frame.
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum EWebSocketOpCode {
    /// A continuation frame.
    k_eWebSocketOpCode_Continuation = 0x00,
    /// A text frame.
    k_eWebSocketOpCode_Text = 0x01,
    /// A binary frame. The only opcode NetHook2 records.
    k_eWebSocketOpCode_Binary = 0x02,
    /// A close frame.
    k_eWebSocketOpCode_Close = 0x08,
    /// A ping frame.
    k_eWebSocketOpCode_Ping = 0x09,
    /// A pong frame.
    k_eWebSocketOpCode_Pong = 0x0A,
}

const WEBSOCKET_OPCODE_NAMES: [&str; 16] = [
    "Continuation",
    "Text",
    "Binary",
    "<Reserved>", // 0x03
    "<Reserved>", // 0x04
    "<Reserved>", // 0x05
    "<Reserved>", // 0x06
    "<Reserved>", // 0x07
    "Close",
    "Ping",
    "Pong",
    "<Reserved>", // 0x0B
    "<Reserved>", // 0x0C
    "<Reserved>", // 0x0D
    "<Reserved>", // 0x0E
    "<Reserved>", // 0x0F
];

/// Returns the human readable name of a WebSocket opcode.
pub fn e_web_socket_op_code_to_name(op_code: EWebSocketOpCode) -> &'static str {
    let value = op_code as i32;

    if value < 0 || value as usize >= WEBSOCKET_OPCODE_NAMES.len() {
        return "<Invalid>";
    }

    WEBSOCKET_OPCODE_NAMES[value as usize]
}

/// A network packet handed to `CCMInterface::RecvPkt`.
///
/// The layout is load bearing: NetHook2 reads `m_pubData` and `m_cubData` straight
/// out of the object the Steam client passes to the hooked function.
#[derive(Clone, Copy, Debug)]
#[allow(non_snake_case)]
#[repr(C)]
pub struct CNetPacket {
    /// The connection the packet belongs to.
    pub m_hConnection: HCONNECTION,
    /// Unknown.
    pub m_unUnk1: u32,
    /// Unknown.
    pub m_unUnk2: u32,
    /// The packet payload.
    pub m_pubData: *mut u8,
    /// The length of the packet payload.
    pub m_cubData: u32,
    /// The reference count.
    pub m_cRef: i32,
    /// The network buffer the packet was read from.
    pub m_pubNetworkBuffer: *mut u8,
}
