//! Steam UDP packet types and message headers.

use super::csteamid::CSteamID;
use super::emsg::EMsg;
use super::steamtypes::JobId;

/// Do not use IOCP for this connection.
pub const K_U_NET_FLAG_NO_IOCP: u32 = 1;

/// Find an available port for this connection.
pub const K_U_NET_FLAG_FIND_AVAIL_PORT: u32 = 2;

/// Use authentication for this connection.
pub const K_U_NET_FLAG_USE_AUTHENTICATION: u32 = 4;

/// Use encryption for this connection.
pub const K_U_NET_FLAG_USE_ENCRYPTION: u32 = 8;

/// The connection is a raw stream.
pub const K_U_NET_FLAG_RAW_STREAM: u32 = 16;

/// The connection sends a raw stream.
pub const K_U_NET_FLAG_RAW_STREAM_SEND: u32 = 32;

/// The socket is not bound.
pub const K_U_NET_FLAG_UNBOUND_SOCKET: u32 = 64;

/// The connection uses raw I/O.
pub const K_U_NET_FLAG_RAW_IO_RECV: u32 = 128;

/// The connection requires the key callback.
pub const K_U_NET_FLAGS_KEY_CALLBACK_REQUIRED: u32 =
    K_U_NET_FLAG_USE_AUTHENTICATION | K_U_NET_FLAG_USE_ENCRYPTION;

/// The type of a UDP packet.
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum EUDPPktType {
    /// The first packet type sent to Steam servers by the client.
    k_EUDPPktTypeChallengeReq = 1,
    /// The server's response to a challenge request.
    k_EUDPPktTypeChallenge = 2,
    /// Sent after the client picked the best Steam server.
    k_EUDPPktTypeConnect = 3,
    /// The server accepts the connection.
    k_EUDPPktTypeAccept = 4,
    /// The connection is being terminated.
    k_EUDPPktTypeDisconnect = 5,
    /// The majority of VS01 traffic.
    k_EUDPPktTypeData = 6,
    /// A packet resend.
    k_EUDPPktTypeDatagram = 7,
    /// The largest valid packet type.
    k_EUDPPktTypeMax = 8,
}

/// The header of a UDP packet.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
#[allow(non_snake_case)]
#[repr(C, packed)]
pub struct UDPPktHdr_t {
    /// The packet magic, "VS01" or "VT01".
    pub m_nMagic: u32,
    /// The size of the packet.
    pub m_cbPkt: u16,
    /// The type of the packet.
    pub m_EUDPPktType: u8,
    /// The network flags.
    pub m_nFlags: u8,
    /// The source connection id.
    pub m_nSrcConnectionID: u32,
    /// The destination connection id.
    pub m_nDstConnectionID: u32,
    /// The sequence number of this packet.
    pub m_nSeqThis: u32,
    /// The sequence number this packet acknowledges.
    pub m_nSeqAcked: u32,
    /// The number of packets in the message.
    pub m_nPktsInMsg: u32,
    /// The sequence number the message starts at.
    pub m_nMsgStartSeq: u32,
    /// The size of the message data.
    pub m_cbMsgData: u32,
}

/// The classic Steam 3 message header.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
#[repr(C, packed)]
#[allow(non_snake_case)]
pub struct MsgHdr_t {
    /// The message type.
    pub m_EMsg: i32,
    /// The job the message targets.
    pub m_JobIDTarget: JobId,
    /// The job the message originates from.
    pub m_JobIDSource: JobId,
}

/// The extended Steam 3 message header.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
#[repr(C, packed)]
#[allow(non_snake_case)]
pub struct ExtendedClientMsgHdr_t {
    /// The message type.
    pub m_EMsg: i32,
    /// The size of the header.
    pub m_nCubHdr: u8,
    /// The header version.
    pub m_nHdrVersion: u16,
    /// The job the message targets.
    pub m_JobIDTarget: JobId,
    /// The job the message originates from.
    pub m_JobIDSource: JobId,
    /// The header canary.
    pub m_nHdrCanary: u8,
    /// The Steam id of the sender.
    pub m_ulSteamID: CSteamID,
    /// The session id.
    pub m_nSessionID: i32,
}

/// Returns the wire value of a message type read out of a message header.
pub const fn emsg_from_raw(value: u16) -> EMsg {
    EMsg::from_raw(value as u32)
}
