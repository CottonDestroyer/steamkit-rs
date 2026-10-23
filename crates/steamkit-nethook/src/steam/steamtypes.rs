//! Basic Steam types: integer aliases, a few constants, and the `EResult` and
//! `EServerType` enums.

use std::ffi::c_void;

/// Valve's `uint64` alias.
pub type SteamUnsigned64 = u64;

/// Valve's `GID_t`, a globally unique id.
pub type Gid = u64;

/// The nil `GID`.
pub const K_GID_NIL: Gid = 0xffff_ffff_ffff_ffff;

/// Each job has a unique id.
pub type JobId = Gid;

/// The nil transaction id.
pub const K_TXN_ID_NIL: Gid = K_GID_NIL;

/// The unknown transaction id.
pub const K_TXN_ID_UNKNOWN: Gid = 0;

/// An application id.
pub type AppId = u32;

/// A package id.
pub type PackageId = u32;

/// A depot id.
pub type DepotId = u32;

/// The invalid application id.
pub const K_U_APP_ID_INVALID: AppId = 0x0;

/// The invalid package id.
pub const K_U_PACKAGE_ID_INVALID: PackageId = 0xFFFF_FFFF;

/// The invalid depot id.
pub const K_U_DEPOT_ID_INVALID: DepotId = 0x0;

/// The invalid cell id.
pub const K_U_CELL_ID_INVALID: u32 = 0xFFFF_FFFF;

/// A handle to a Steam API call.
pub type SteamApiCall = u64;

/// The invalid Steam API call handle.
pub const K_U_API_CALL_INVALID: SteamApiCall = 0x0;

/// A handle to a communication pipe to the Steam client.
pub type HSteamPipe = i32;

/// A handle to a single Steam user.
pub type HSteamUser = i32;

/// A handle to a Steam call.
pub type HSteamCall = i32;

/// A handle returned by `GetAuthSessionTicket`.
pub type HAuthTicket = u32;

/// The invalid auth ticket handle.
pub const K_H_AUTH_TICKET_INVALID: HAuthTicket = 0;

/// The maximum length of a system IM message.
pub const K_CCH_SYSTEM_IM_TEXT_MAX: usize = 4096;

/// A 32 bit timestamp with one second resolution since the Unix epoch.
pub type RTime32 = u32;

/// The nil `RTime32`.
pub const K_RTIME32_NIL: RTime32 = 0;

/// The smallest valid `RTime32`.
pub const K_RTIME32_MIN_VALID: RTime32 = 10;

/// The infinite `RTime32`.
pub const K_RTIME32_INFINITE: RTime32 = 0x7FFF_FFFF;

/// The magic number that starts a Steam network packet ("VS01").
pub const K_N_MAGIC: u32 = 0x3130_5356;

/// The old magic number that starts a Steam 2 network packet ("VSTM").
pub const K_N_MAGIC_OLD1: u32 = 0x4D54_5356;

/// The maximum length of a truncated password.
pub const K_CCH_TRUNCATED_PASSWORD: usize = 20;

/// The maximum length of an account name.
pub const K_CCH_ACCOUNT_NAME: usize = 64;

/// The mask used to derive a UDP challenge.
pub const K_N_CHALLENGE_MASK: u32 = 0xA426_DF2B;

/// The mask used to obfuscate a private IP address.
pub const K_N_OBFUSCATION_MASK: u32 = 0xBAAD_F00D;

/// The signature of Valve's `CreateInterface` factory function.
pub type CreateInterfaceFn = unsafe extern "C" fn(*const i8, *mut i32) -> *mut c_void;

/// The general result codes returned by the Steam client.
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum EResult {
    /// Success.
    k_EResultOK = 1,
    /// Generic failure.
    k_EResultFail = 2,
    /// No connection to the Steam network.
    k_EResultNoConnection = 3,
    /// The password or ticket is invalid.
    k_EResultInvalidPassword = 5,
    /// The user is logged in elsewhere.
    k_EResultLoggedInElsewhere = 6,
    /// The protocol version is incorrect.
    k_EResultInvalidProtocolVer = 7,
    /// A parameter is incorrect.
    k_EResultInvalidParam = 8,
    /// The file was not found.
    k_EResultFileNotFound = 9,
    /// The called method is busy.
    k_EResultBusy = 10,
    /// The called object was in an invalid state.
    k_EResultInvalidState = 11,
    /// The name is invalid.
    k_EResultInvalidName = 12,
    /// The email is invalid.
    k_EResultInvalidEmail = 13,
    /// The name is not unique.
    k_EResultDuplicateName = 14,
    /// Access is denied.
    k_EResultAccessDenied = 15,
    /// The operation timed out.
    k_EResultTimeout = 16,
    /// The account is banned.
    k_EResultBanned = 17,
    /// The account was not found.
    k_EResultAccountNotFound = 18,
    /// The SteamID is invalid.
    k_EResultInvalidSteamID = 19,
    /// The requested service is unavailable.
    k_EResultServiceUnavailable = 20,
    /// The user is not logged on.
    k_EResultNotLoggedOn = 21,
    /// The request is pending.
    k_EResultPending = 22,
    /// Encryption or decryption failed.
    k_EResultEncryptionFailure = 23,
    /// Insufficient privilege.
    k_EResultInsufficientPrivilege = 24,
    /// The rate limit was exceeded.
    k_EResultLimitExceeded = 25,
    /// Access has been revoked.
    k_EResultRevoked = 26,
    /// The license or guest pass has expired.
    k_EResultExpired = 27,
    /// The guest pass was already redeemed.
    k_EResultAlreadyRedeemed = 28,
    /// The request is a duplicate.
    k_EResultDuplicateRequest = 29,
    /// The account already owns the game.
    k_EResultAlreadyOwned = 30,
    /// The IP address was not found.
    k_EResultIPNotFound = 31,
    /// Failed to write change to the data store.
    k_EResultPersistFailed = 32,
    /// Failed to acquire access lock for this operation.
    k_EResultLockingFailed = 33,
    /// The logon session was replaced.
    k_EResultLogonSessionReplaced = 34,
    /// The connection failed.
    k_EResultConnectFailed = 35,
    /// The handshake failed.
    k_EResultHandshakeFailed = 36,
    /// An I/O failure occurred.
    k_EResultIOFailure = 37,
    /// The remote end disconnected.
    k_EResultRemoteDisconnect = 38,
    /// The shopping cart was not found.
    k_EResultShoppingCartNotFound = 39,
    /// The user did not allow this.
    k_EResultBlocked = 40,
    /// The target is ignoring the sender.
    k_EResultIgnored = 41,
    /// No matching request was found.
    k_EResultNoMatch = 42,
    /// The account is disabled.
    k_EResultAccountDisabled = 43,
    /// The service is read only.
    k_EResultServiceReadOnly = 44,
    /// The account is not featured.
    k_EResultAccountNotFeatured = 45,
    /// The requester is an administrator.
    k_EResultAdministratorOK = 46,
    /// The content version does not match.
    k_EResultContentVersion = 47,
    /// The current CM cannot service the user.
    k_EResultTryAnotherCM = 48,
    /// The session is already logged in elsewhere.
    k_EResultAlreadyLoggedInElsewhere = 50,
    /// The account is suspended.
    k_EResultSuspended = 51,
    /// The operation was cancelled.
    k_EResultCancelled = 52,
    /// The data is corrupt.
    k_EResultDataCorruption = 53,
    /// The disk is full.
    k_EResultDiskFull = 54,
    /// The remote call failed.
    k_EResultRemoteCallFailed = 55,
}

/// The kind of server a message is addressed to.
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum EServerType {
    /// An invalid server type.
    k_EServerTypeInvalid = -1,
    /// The shell.
    k_EServerTypeShell = 0,
    /// The game master.
    k_EServerTypeGM = 1,
    /// The beta updater manager.
    k_EServerTypeBUM = 2,
    /// The account manager.
    k_EServerTypeAM = 3,
    /// The beta server.
    k_EServerTypeBS = 4,
    /// The voice server.
    k_EServerTypeVS = 5,
    /// The anti-cheat server.
    k_EServerTypeATS = 6,
    /// The connection manager.
    k_EServerTypeCM = 7,
    /// The friend build server.
    k_EServerTypeFBS = 8,
    /// The friends game server.
    k_EServerTypeFG = 9,
    /// The Steam server.
    k_EServerTypeSS = 10,
    /// The DRM server.
    k_EServerTypeDRMS = 11,
    /// The (obsolete) hub server.
    k_EServerTypeHubOBSOLETE = 12,
    /// The console server.
    k_EServerTypeConsole = 13,
    /// The (obsolete) app server.
    k_EServerTypeASBOBSOLETE = 14,
    /// The client server.
    k_EServerTypeClient = 15,
    /// The (obsolete) bootstrap server.
    k_EServerTypeBootstrapOBSOLETE = 16,
    /// The download proxy.
    k_EServerTypeDP = 17,
    /// The web generator.
    k_EServerTypeWG = 18,
    /// The server manager.
    k_EServerTypeSM = 19,
    /// The UFS server.
    k_EServerTypeUFS = 21,
    /// The utility server.
    k_EServerTypeUtil = 23,
    /// The download server.
    k_EServerTypeDSS = 24,
    /// The (obsolete) P2P relay.
    k_EServerTypeP2PRelayOBSOLETE = 25,
    /// The app information server.
    k_EServerTypeAppInformation = 26,
    /// A spare server type.
    k_EServerTypeSpare = 27,
    /// The file transfer server.
    k_EServerTypeFTS = 28,
    /// The EULA proxy manager.
    k_EServerTypeEPM = 29,
    /// The peer to peer server.
    k_EServerTypePS = 30,
    /// The inventory server.
    k_EServerTypeIS = 31,
    /// The cheat server.
    k_EServerTypeCCS = 32,
    /// The dedicated server farm.
    k_EServerTypeDFS = 33,
    /// The load balancer server.
    k_EServerTypeLBS = 34,
    /// The master server directory.
    k_EServerTypeMDS = 35,
    /// The content server.
    k_EServerTypeCS = 36,
    /// The game coordinator.
    k_EServerTypeGC = 37,
    /// The name server.
    k_EServerTypeNS = 38,
    /// The online game services.
    k_EServerTypeOGS = 39,
    /// The Web API.
    k_EServerTypeWebAPI = 40,
    /// The user data store.
    k_EServerTypeUDS = 41,
    /// The matchmaking server.
    k_EServerTypeMMS = 42,
    /// The game server manager.
    k_EServerTypeGMS = 43,
    /// The key generator.
    k_EServerTypeKGS = 44,
    /// The unified connection manager.
    k_EServerTypeUCM = 45,
    /// The remote manager.
    k_EServerTypeRM = 46,
    /// The file server.
    k_EServerTypeFS = 47,
    /// The economy server.
    k_EServerTypeEcon = 48,
    /// The backpack server.
    k_EServerTypeBackpack = 49,
    /// The largest valid server type.
    k_EServerTypeMax = 50,
}

/// A Steam instance id; must be 16 bits wide.
pub type SteamInstanceID = u16;

/// A local Steam user id; must be 64 bits wide.
pub type SteamLocalUserID = u64;

/// The low and high halves of a split local user id.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[allow(non_snake_case)]
pub struct TSteamSplitLocalUserID {
    /// The low 32 bits.
    pub Low32bits: u32,
    /// The high 32 bits.
    pub High32bits: u32,
}

/// The Steam 2 flavour of a global user id.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[allow(non_snake_case)]
pub struct TSteamGlobalUserID {
    /// The instance the user belongs to.
    pub m_SteamInstanceID: SteamInstanceID,
    /// The local user id.
    pub m_SteamLocalUserID: SteamLocalUserID,
}

/// A callback message as it is handed to the Steam client.
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug)]
#[allow(non_snake_case)]
pub struct CallbackMsg_t {
    /// The user the callback belongs to.
    pub m_hSteamUser: HSteamUser,
    /// The callback id.
    pub m_iCallback: i32,
    /// The callback payload.
    pub m_pubParam: *mut u8,
    /// The size of the callback payload.
    pub m_cubParam: i32,
}
