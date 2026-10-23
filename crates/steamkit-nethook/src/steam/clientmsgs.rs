//! Steam client message structs.

use super::csteamid::CSteamID;
use super::steamtypes::RTime32;

/// The request to negotiate channel encryption.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
#[allow(non_snake_case)]
#[repr(C, packed)]
pub struct MsgChannelEncryptRequest_t {
    /// The protocol version the client speaks.
    pub m_unProtocolVer: u32,
    /// The universe the client belongs to.
    pub m_EUniverse: i32,
}

/// The response to a channel encryption request.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
#[allow(non_snake_case)]
#[repr(C, packed)]
pub struct MsgChannelEncryptResponse_t {
    /// The protocol version the server speaks.
    pub m_unProtocolVer: u32,
    /// The length of the encrypted key.
    pub m_cubEncryptedKey: u32,
}

/// The result of a channel encryption attempt.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
#[allow(non_snake_case)]
#[repr(C, packed)]
pub struct MsgChannelEncryptResult_t {
    /// The result of the encryption attempt.
    pub m_EResult: i32,
}

/// The header of a multiplexed message batch.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
#[allow(non_snake_case)]
#[repr(C, packed)]
pub struct MsgMulti_t {
    /// The size of the decompressed batch.
    pub m_cubUnzipped: u32,
}

/// The anonymous logon request.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
#[allow(non_snake_case)]
#[repr(C, packed)]
pub struct MsgClientAnonLogOn_t {
    /// The protocol version the client speaks.
    pub m_unProtocolVer: u32,
    /// The obfuscated private IP address.
    pub m_unIPPrivateObfuscated: u32,
    /// The public IP address.
    pub m_unIPPublic: u32,
}

/// The response to a logon request.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
#[allow(non_snake_case)]
#[repr(C, packed)]
pub struct MsgClientLogOnResponse_t {
    /// The result of the logon.
    pub m_EResult: i32,
    /// The out of game heartbeat rate.
    pub m_nOutOfGameHeartbeatRateSec: i32,
    /// The in game heartbeat rate.
    pub m_nInGameHeartbeatRateSec: i32,
    /// The Steam id the client supplied.
    pub m_ulClientSuppliedSteamId: CSteamID,
    /// The public IP address of the client.
    pub m_unIPPublic: u32,
    /// The server's real time.
    pub m_RTime32ServerRealTime: RTime32,
}

/// The logon request that carries credentials.
#[derive(Clone, Copy)]
#[allow(non_snake_case)]
#[repr(C, packed)]
pub struct MsgClientLogOnWithCredentials_t {
    /// The protocol version the client speaks.
    pub m_unProtocolVer: u32,
    /// The obfuscated private IP address.
    pub m_unIPPrivateObfuscated: u32,
    /// The public IP address.
    pub m_unIPPublic: u32,
    /// The Steam id the client supplied.
    pub m_ulClientSuppliedSteamId: u64,
    /// The length of the ticket.
    pub m_unTicketLength: u32,
    /// The account name.
    pub m_rgchAccountName: [u8; 64],
    /// The password.
    pub m_rgchPassword: [u8; 20],
    /// The quality of service level.
    pub m_qosLevel: u32,
}

/// The request to register an auth ticket with a connection manager.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
#[allow(non_snake_case)]
#[repr(C, packed)]
pub struct MsgClientRegisterAuthTicketWithCM_t {
    /// The protocol version the client speaks.
    pub m_unProtocolVer: u32,
    /// The length of the ticket including its signature.
    pub m_unTicketLengthWithSignature: u32,
}
