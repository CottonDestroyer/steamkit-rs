//! Callbacks posted by [`SteamApps`](super::steam_apps::SteamApps).
//!
//! Maps are `BTreeMap`s so iteration order is stable. `ELicenseFlags` is a bit
//! set, but the generated enum can only hold one value, so the raw bits are
//! kept too (see [`License::license_flags_value`]).

use std::any::Any;
use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use chrono::{DateTime, Utc};
use steamkit_protos::steam::{
    c_msg_client_license_list, c_msg_client_pics_product_info_response,
    CMsgClientCheckAppBetaPasswordResponse, CMsgClientGameConnectTokens,
    CMsgClientGetAppOwnershipTicketResponse, CMsgClientGetDepotDecryptionKeyResponse,
    CMsgClientLicenseList, CMsgClientPicsAccessTokenResponse, CMsgClientPicsChangesSinceResponse,
    CMsgClientPicsPrivateBetaResponse, CMsgClientPicsProductInfoResponse,
    CMsgClientPurchaseResponse, CMsgClientRedeemGuestPassResponse,
    CMsgClientRequestFreeLicenseResponse,
};

use crate::base::client_msg::{ClientMsg, ClientMsgProtobuf, IClientMsg};
use crate::base::generated::steam_language::internal::{
    MsgClientGetLegacyGameKeyResponse, MsgClientUpdateGuestPassesList, MsgClientVACBanStatus,
};
use crate::base::generated::steam_language::{
    ELicenseFlags, ELicenseType, EPaymentMethod, EPurchaseResultDetail, EResult,
};
use crate::base::msg_base::MsgBase;
use crate::base::packet_base::IPacketMsg;
use crate::error::SteamResult;
use crate::steam::steam_client::callback_mgr::callback_msg::{CallbackMsg, CallbackMsgBase};
use crate::types::job_id::JobID;
use crate::types::key_value::KeyValue;

/// What an absent `eresult` field defaults to in the Steam protocol.
const ERESULT_DEFAULT: i32 = 2;

/// Converts an optional `eresult` into an [`EResult`].
fn eresult_from_i32(value: Option<i32>) -> EResult {
    EResult::from_value(value.unwrap_or(ERESULT_DEFAULT)).unwrap_or(EResult::Invalid)
}

/// Converts an optional `eresult` into an [`EResult`].
fn eresult_from_u32(value: Option<u32>) -> EResult {
    EResult::from_value(value.unwrap_or(ERESULT_DEFAULT as u32) as i32).unwrap_or(EResult::Invalid)
}

/// Converts a Unix timestamp to a UTC date time.
fn date_time_from_unix_time(seconds: u64) -> DateTime<Utc> {
    match crate::util::date_utils::DateUtils::date_time_from_unix_time(seconds) {
        Ok(time) => DateTime::<Utc>::from(time),
        Err(_) => DateTime::<Utc>::from(std::time::UNIX_EPOCH),
    }
}

/// A [`Read`] adapter over the payload of a client message.
///
/// It hands out one byte per call on purpose: the binary [`KeyValue`] reader
/// buffers 4 KiB at a time, which would move the payload position way past the
/// entry it parsed and break every entry after the first.
struct PayloadReader<'a> {
    payload: &'a mut MsgBase,
}

impl Read for PayloadReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let start = (self.payload.position as usize).min(self.payload.payload.len());
        if start >= self.payload.payload.len() {
            return Ok(0);
        }

        buf[0] = self.payload.payload[start];
        self.payload.position += 1;
        Ok(1)
    }
}

/// Posted during logon with the account's licenses, and again whenever the
/// license list changes (a purchase, a redeemed key, a refund, ...).
#[derive(Clone, Debug)]
pub struct LicenseListCallback {
    base: CallbackMsgBase,
    result: EResult,
    license_list: Vec<License>,
}

impl LicenseListCallback {
    /// Builds the callback from a received `ClientLicenseList` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let license_list = ClientMsgProtobuf::<CMsgClientLicenseList>::from_packet(packet_msg)?;
        let body = license_list.body();

        let licenses = body.licenses.iter().map(License::from_proto).collect();

        Ok(Self {
            base: CallbackMsgBase::new(),
            result: eresult_from_i32(body.eresult),
            license_list: licenses,
        })
    }

    /// The result of the message.
    pub fn result(&self) -> EResult {
        self.result
    }

    /// The license list.
    pub fn license_list(&self) -> &[License] {
        &self.license_list
    }
}

impl CallbackMsg for LicenseListCallback {
    fn job_id(&self) -> JobID {
        self.base.job_id()
    }

    fn set_job_id(&mut self, id: JobID) {
        self.base.set_job_id(id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// A granted license (steam3 subscription) for one or more games.
#[derive(Clone, Debug)]
pub struct License {
    package_id: u32,
    last_change_number: i32,
    time_created: DateTime<Utc>,
    time_next_process: DateTime<Utc>,
    minute_limit: i32,
    minutes_used: i32,
    payment_method: EPaymentMethod,
    payment_method_value: u32,
    license_flags: ELicenseFlags,
    license_flags_value: u32,
    purchase_country_code: String,
    license_type: ELicenseType,
    territory_code: i32,
    owner_account_id: u32,
    access_token: u64,
    master_package_id: u32,
}

impl License {
    /// Builds a license from its protobuf form.
    fn from_proto(license: &c_msg_client_license_list::License) -> Self {
        let payment_method_value = license.payment_method.unwrap_or_default();
        let license_flags_value = license.flags.unwrap_or_default();

        Self {
            package_id: license.package_id.unwrap_or_default(),
            last_change_number: license.change_number.unwrap_or_default(),
            time_created: date_time_from_unix_time(u64::from(
                license.time_created.unwrap_or_default(),
            )),
            time_next_process: date_time_from_unix_time(u64::from(
                license.time_next_process.unwrap_or_default(),
            )),
            minute_limit: license.minute_limit.unwrap_or_default(),
            minutes_used: license.minutes_used.unwrap_or_default(),
            payment_method: EPaymentMethod::from_value(payment_method_value as i32)
                .unwrap_or(EPaymentMethod::None),
            payment_method_value,
            license_flags: ELicenseFlags::from_value(license_flags_value as i32)
                .unwrap_or(ELicenseFlags::None),
            license_flags_value,
            purchase_country_code: license.purchase_country_code.clone().unwrap_or_default(),
            license_type: ELicenseType::from_value(license.license_type.unwrap_or_default() as i32)
                .unwrap_or(ELicenseType::NoLicense),
            territory_code: license.territory_code.unwrap_or_default(),
            owner_account_id: license.owner_id.unwrap_or_default(),
            access_token: license.access_token.unwrap_or_default(),
            master_package_id: license.master_package_id.unwrap_or_default(),
        }
    }

    /// The package ID used to identify the license.
    pub fn package_id(&self) -> u32 {
        self.package_id
    }

    /// The last change number for this license.
    pub fn last_change_number(&self) -> i32 {
        self.last_change_number
    }

    /// The time the license was created.
    pub fn time_created(&self) -> DateTime<Utc> {
        self.time_created
    }

    /// The next process time for the license.
    pub fn time_next_process(&self) -> DateTime<Utc> {
        self.time_next_process
    }

    /// The minute limit of the license.
    pub fn minute_limit(&self) -> i32 {
        self.minute_limit
    }

    /// The minutes used of the license.
    pub fn minutes_used(&self) -> i32 {
        self.minutes_used
    }

    /// The payment method used when the license was created.
    pub fn payment_method(&self) -> EPaymentMethod {
        self.payment_method
    }

    /// The raw value of the payment method.
    pub fn payment_method_value(&self) -> u32 {
        self.payment_method_value
    }

    /// The license flags.
    pub fn license_flags(&self) -> ELicenseFlags {
        self.license_flags
    }

    /// The raw bits of the license flags.
    pub fn license_flags_value(&self) -> u32 {
        self.license_flags_value
    }

    /// The two letter country code where the license was purchased.
    pub fn purchase_country_code(&self) -> &str {
        &self.purchase_country_code
    }

    /// The type of the license.
    pub fn license_type(&self) -> ELicenseType {
        self.license_type
    }

    /// The territory code of the license.
    pub fn territory_code(&self) -> i32 {
        self.territory_code
    }

    /// The owner account id of the license.
    pub fn owner_account_id(&self) -> u32 {
        self.owner_account_id
    }

    /// The PICS access token for this package.
    pub fn access_token(&self) -> u64 {
        self.access_token
    }

    /// The master package id.
    pub fn master_package_id(&self) -> u32 {
        self.master_package_id
    }
}

/// This callback is received in response to requesting a free license.
#[derive(Clone, Debug)]
pub struct FreeLicenseCallback {
    base: CallbackMsgBase,
    result: EResult,
    granted_apps: Vec<u32>,
    granted_packages: Vec<u32>,
}

impl FreeLicenseCallback {
    /// Builds the callback from a received `ClientRequestFreeLicenseResponse` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let granted =
            ClientMsgProtobuf::<CMsgClientRequestFreeLicenseResponse>::from_packet(packet_msg)?;
        let body = granted.body();

        Ok(Self {
            base: CallbackMsgBase::with_job_id(granted.target_job_id()),
            result: eresult_from_u32(body.eresult),
            granted_apps: body.granted_appids.clone(),
            granted_packages: body.granted_packageids.clone(),
        })
    }

    /// The result of the message.
    pub fn result(&self) -> EResult {
        self.result
    }

    /// The list of granted apps.
    pub fn granted_apps(&self) -> &[u32] {
        &self.granted_apps
    }

    /// The list of granted packages.
    pub fn granted_packages(&self) -> &[u32] {
        &self.granted_packages
    }
}

impl CallbackMsg for FreeLicenseCallback {
    fn job_id(&self) -> JobID {
        self.base.job_id()
    }

    fn set_job_id(&mut self, id: JobID) {
        self.base.set_job_id(id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
/// This callback is received in response to requesting an app ownership ticket.
#[derive(Clone, Debug)]
pub struct AppOwnershipTicketCallback {
    base: CallbackMsgBase,
    result: EResult,
    app_id: u32,
    ticket: Vec<u8>,
}

impl AppOwnershipTicketCallback {
    /// Builds the callback from a received `ClientGetAppOwnershipTicketResponse` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let response =
            ClientMsgProtobuf::<CMsgClientGetAppOwnershipTicketResponse>::from_packet(packet_msg)?;
        let body = response.body();

        Ok(Self {
            base: CallbackMsgBase::with_job_id(response.target_job_id()),
            result: eresult_from_u32(body.eresult),
            app_id: body.app_id.unwrap_or_default(),
            ticket: body.ticket.clone().unwrap_or_default(),
        })
    }

    /// The result of requesting the ticket.
    pub fn result(&self) -> EResult {
        self.result
    }

    /// The app id this ticket is for.
    pub fn app_id(&self) -> u32 {
        self.app_id
    }

    /// The ticket data.
    pub fn ticket(&self) -> &[u8] {
        &self.ticket
    }
}

impl CallbackMsg for AppOwnershipTicketCallback {
    fn job_id(&self) -> JobID {
        self.base.job_id()
    }

    fn set_job_id(&mut self, id: JobID) {
        self.base.set_job_id(id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// This callback is received in response to requesting a depot decryption key.
#[derive(Clone, Debug)]
pub struct DepotKeyCallback {
    base: CallbackMsgBase,
    result: EResult,
    depot_id: u32,
    depot_key: Vec<u8>,
}

impl DepotKeyCallback {
    /// Builds the callback from a received `ClientGetDepotDecryptionKeyResponse` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let response =
            ClientMsgProtobuf::<CMsgClientGetDepotDecryptionKeyResponse>::from_packet(packet_msg)?;
        let body = response.body();

        Ok(Self {
            base: CallbackMsgBase::with_job_id(response.target_job_id()),
            result: eresult_from_i32(body.eresult),
            depot_id: body.depot_id.unwrap_or_default(),
            depot_key: body.depot_encryption_key.clone().unwrap_or_default(),
        })
    }

    /// The result of requesting this encryption key.
    pub fn result(&self) -> EResult {
        self.result
    }

    /// The depot id this encryption key is for.
    pub fn depot_id(&self) -> u32 {
        self.depot_id
    }

    /// The encryption key for this depot.
    pub fn depot_key(&self) -> &[u8] {
        &self.depot_key
    }
}

impl CallbackMsg for DepotKeyCallback {
    fn job_id(&self) -> JobID {
        self.base.job_id()
    }

    fn set_job_id(&mut self, id: JobID) {
        self.base.set_job_id(id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Decodes bytes as ASCII; anything above `0x7F` becomes `?`.
fn ascii_decode(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| if *byte < 0x80 { *byte as char } else { '?' })
        .collect()
}

/// This callback is received in response to requesting a legacy game key.
#[derive(Clone, Debug)]
pub struct LegacyGameKeyCallback {
    base: CallbackMsgBase,
    result: EResult,
    app_id: u32,
    key: Option<String>,
}

impl LegacyGameKeyCallback {
    /// Builds the callback from a received `ClientGetLegacyGameKeyResponse` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let response = ClientMsg::<MsgClientGetLegacyGameKeyResponse>::from_packet(packet_msg)?;
        let body = response.body();

        let key = if body.length > 0 {
            let payload = response.payload().to_bytes();
            let length = (body.length as usize).saturating_sub(1).min(payload.len());
            Some(ascii_decode(&payload[..length]))
        } else {
            None
        };

        Ok(Self {
            base: CallbackMsgBase::with_job_id(response.target_job_id()),
            result: body.result,
            app_id: body.app_id,
            key,
        })
    }

    /// The result of requesting this game key.
    pub fn result(&self) -> EResult {
        self.result
    }

    /// The app id that this game key is for.
    pub fn app_id(&self) -> u32 {
        self.app_id
    }

    /// The game key.
    pub fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }
}

impl CallbackMsg for LegacyGameKeyCallback {
    fn job_id(&self) -> JobID {
        self.base.job_id()
    }

    fn set_job_id(&mut self, id: JobID) {
        self.base.set_job_id(id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// This callback is fired when the client receives a list of game connect tokens.
#[derive(Clone, Debug)]
pub struct GameConnectTokensCallback {
    base: CallbackMsgBase,
    tokens_to_keep: u32,
    tokens: Vec<Vec<u8>>,
}

impl GameConnectTokensCallback {
    /// Builds the callback from a received `ClientGameConnectTokens` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let tokens = ClientMsgProtobuf::<CMsgClientGameConnectTokens>::from_packet(packet_msg)?;
        let body = tokens.body();

        Ok(Self {
            base: CallbackMsgBase::new(),
            tokens_to_keep: body.max_tokens_to_keep.unwrap_or_default(),
            tokens: body.tokens.clone(),
        })
    }

    /// The count of tokens to keep.
    pub fn tokens_to_keep(&self) -> u32 {
        self.tokens_to_keep
    }

    /// The list of tokens.
    pub fn tokens(&self) -> &[Vec<u8>] {
        &self.tokens
    }
}

impl CallbackMsg for GameConnectTokensCallback {
    fn job_id(&self) -> JobID {
        self.base.job_id()
    }

    fn set_job_id(&mut self, id: JobID) {
        self.base.set_job_id(id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// This callback is fired when the client receives its VAC banned status.
#[derive(Clone, Debug)]
pub struct VACStatusCallback {
    base: CallbackMsgBase,
    banned_apps: Vec<u32>,
}

impl VACStatusCallback {
    /// Builds the callback from a received `ClientVACBanStatus` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let vac_status = ClientMsg::<MsgClientVACBanStatus>::from_packet(packet_msg)?;
        let num_bans = vac_status.body().num_bans as usize;

        let mut banned_apps = Vec::with_capacity(num_bans);
        let mut reader = vac_status.payload().reader();

        for _ in 0..num_bans {
            banned_apps.push(reader.read_u32()?);
        }

        Ok(Self {
            base: CallbackMsgBase::new(),
            banned_apps,
        })
    }

    /// The list of VAC banned apps the client is banned from.
    pub fn banned_apps(&self) -> &[u32] {
        &self.banned_apps
    }
}

impl CallbackMsg for VACStatusCallback {
    fn job_id(&self) -> JobID {
        self.base.job_id()
    }

    fn set_job_id(&mut self, id: JobID) {
        self.base.set_job_id(id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Posted when PICS returns access tokens for a list of app and package ids.
#[derive(Clone, Debug)]
pub struct PICSTokensCallback {
    base: CallbackMsgBase,
    package_tokens_denied: Vec<u32>,
    app_tokens_denied: Vec<u32>,
    package_tokens: BTreeMap<u32, u64>,
    app_tokens: BTreeMap<u32, u64>,
}

impl PICSTokensCallback {
    /// Builds the callback from a received `ClientPICSAccessTokenResponse` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let response =
            ClientMsgProtobuf::<CMsgClientPicsAccessTokenResponse>::from_packet(packet_msg)?;
        let body = response.body();

        let mut package_tokens = BTreeMap::new();
        for token in &body.package_access_tokens {
            package_tokens.insert(
                token.packageid.unwrap_or_default(),
                token.access_token.unwrap_or_default(),
            );
        }

        let mut app_tokens = BTreeMap::new();
        for token in &body.app_access_tokens {
            app_tokens.insert(
                token.appid.unwrap_or_default(),
                token.access_token.unwrap_or_default(),
            );
        }

        Ok(Self {
            base: CallbackMsgBase::with_job_id(response.target_job_id()),
            package_tokens_denied: body.package_denied_tokens.clone(),
            app_tokens_denied: body.app_denied_tokens.clone(),
            package_tokens,
            app_tokens,
        })
    }

    /// The list of denied package tokens.
    pub fn package_tokens_denied(&self) -> &[u32] {
        &self.package_tokens_denied
    }

    /// The list of denied app tokens.
    pub fn app_tokens_denied(&self) -> &[u32] {
        &self.app_tokens_denied
    }

    /// The requested package tokens.
    pub fn package_tokens(&self) -> &BTreeMap<u32, u64> {
        &self.package_tokens
    }

    /// The requested app tokens.
    pub fn app_tokens(&self) -> &BTreeMap<u32, u64> {
        &self.app_tokens
    }
}

impl CallbackMsg for PICSTokensCallback {
    fn job_id(&self) -> JobID {
        self.base.job_id()
    }

    fn set_job_id(&mut self, id: JobID) {
        self.base.set_job_id(id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
/// Posted when PICS returns the changes since a given change number.
#[derive(Clone, Debug)]
pub struct PICSChangesCallback {
    base: CallbackMsgBase,
    last_change_number: u32,
    current_change_number: u32,
    requires_full_update: bool,
    requires_full_app_update: bool,
    requires_full_package_update: bool,
    package_changes: BTreeMap<u32, PICSChangeData>,
    app_changes: BTreeMap<u32, PICSChangeData>,
}

impl PICSChangesCallback {
    /// Builds the callback from a received `ClientPICSChangesSinceResponse` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let response =
            ClientMsgProtobuf::<CMsgClientPicsChangesSinceResponse>::from_packet(packet_msg)?;
        let body = response.body();

        let mut package_changes = BTreeMap::new();
        for change in &body.package_changes {
            package_changes.insert(
                change.packageid.unwrap_or_default(),
                PICSChangeData::new(
                    change.packageid.unwrap_or_default(),
                    change.change_number.unwrap_or_default(),
                    change.needs_token.unwrap_or_default(),
                ),
            );
        }

        let mut app_changes = BTreeMap::new();
        for change in &body.app_changes {
            app_changes.insert(
                change.appid.unwrap_or_default(),
                PICSChangeData::new(
                    change.appid.unwrap_or_default(),
                    change.change_number.unwrap_or_default(),
                    change.needs_token.unwrap_or_default(),
                ),
            );
        }

        Ok(Self {
            base: CallbackMsgBase::with_job_id(response.target_job_id()),
            last_change_number: body.since_change_number.unwrap_or_default(),
            current_change_number: body.current_change_number.unwrap_or_default(),
            requires_full_update: body.force_full_update.unwrap_or_default(),
            requires_full_app_update: body.force_full_app_update.unwrap_or_default(),
            requires_full_package_update: body.force_full_package_update.unwrap_or_default(),
            package_changes,
            app_changes,
        })
    }

    /// The change number that was supplied with the request.
    pub fn last_change_number(&self) -> u32 {
        self.last_change_number
    }

    /// The current change number.
    pub fn current_change_number(&self) -> u32 {
        self.current_change_number
    }

    /// Whether this update requires a full update of the information.
    pub fn requires_full_update(&self) -> bool {
        self.requires_full_update
    }

    /// Whether this update requires a full update of the app information.
    pub fn requires_full_app_update(&self) -> bool {
        self.requires_full_app_update
    }

    /// Whether this update requires a full update of the package information.
    pub fn requires_full_package_update(&self) -> bool {
        self.requires_full_package_update
    }

    /// The package changes.
    pub fn package_changes(&self) -> &BTreeMap<u32, PICSChangeData> {
        &self.package_changes
    }

    /// The app changes.
    pub fn app_changes(&self) -> &BTreeMap<u32, PICSChangeData> {
        &self.app_changes
    }
}

impl CallbackMsg for PICSChangesCallback {
    fn job_id(&self) -> JobID {
        self.base.job_id()
    }

    fn set_job_id(&mut self, id: JobID) {
        self.base.set_job_id(id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The change data of a single app or package.
#[derive(Clone, Copy, Debug)]
pub struct PICSChangeData {
    id: u32,
    change_number: u32,
    needs_token: bool,
}

impl PICSChangeData {
    /// Creates a change data entry.
    fn new(id: u32, change_number: u32, needs_token: bool) -> Self {
        Self {
            id,
            change_number,
            needs_token,
        }
    }

    /// The app or package id this change data represents.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// The current change number of this app or package.
    pub fn change_number(&self) -> u32 {
        self.change_number
    }

    /// Whether an access token is needed for this request.
    pub fn needs_token(&self) -> bool {
        self.needs_token
    }
}

/// Posted when PICS returns requested product information.
#[derive(Clone, Debug)]
pub struct PICSProductInfoCallback {
    base: CallbackMsgBase,
    meta_data_only: bool,
    response_pending: bool,
    unknown_packages: Vec<u32>,
    unknown_apps: Vec<u32>,
    apps: BTreeMap<u32, PICSProductInfo>,
    packages: BTreeMap<u32, PICSProductInfo>,
}

impl PICSProductInfoCallback {
    /// Builds the callback from a received `ClientPICSProductInfoResponse` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let response =
            ClientMsgProtobuf::<CMsgClientPicsProductInfoResponse>::from_packet(packet_msg)?;
        let body = response.body();

        let http_host = body.http_host.clone().unwrap_or_default();
        let http_min_size = body.http_min_size.unwrap_or_default();

        let mut packages = BTreeMap::new();
        for package in &body.packages {
            packages.insert(
                package.packageid.unwrap_or_default(),
                PICSProductInfo::from_package(package),
            );
        }

        let mut apps = BTreeMap::new();
        for app in &body.apps {
            apps.insert(
                app.appid.unwrap_or_default(),
                PICSProductInfo::from_app(app, &http_host, http_min_size),
            );
        }

        Ok(Self {
            base: CallbackMsgBase::with_job_id(response.target_job_id()),
            meta_data_only: body.meta_data_only.unwrap_or_default(),
            response_pending: body.response_pending.unwrap_or_default(),
            unknown_packages: body.unknown_packageids.clone(),
            unknown_apps: body.unknown_appids.clone(),
            apps,
            packages,
        })
    }

    /// Whether this response contains only product metadata.
    pub fn meta_data_only(&self) -> bool {
        self.meta_data_only
    }

    /// Whether there are more product information responses pending.
    pub fn response_pending(&self) -> bool {
        self.response_pending
    }

    /// The list of unknown package ids.
    pub fn unknown_packages(&self) -> &[u32] {
        &self.unknown_packages
    }

    /// The list of unknown app ids.
    pub fn unknown_apps(&self) -> &[u32] {
        &self.unknown_apps
    }

    /// The requested app info.
    pub fn apps(&self) -> &BTreeMap<u32, PICSProductInfo> {
        &self.apps
    }

    /// The requested package info.
    pub fn packages(&self) -> &BTreeMap<u32, PICSProductInfo> {
        &self.packages
    }
}

impl CallbackMsg for PICSProductInfoCallback {
    fn job_id(&self) -> JobID {
        self.base.job_id()
    }

    fn set_job_id(&mut self, id: JobID) {
        self.base.set_job_id(id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The information of a single app or package.
#[derive(Clone, Debug)]
pub struct PICSProductInfo {
    id: u32,
    change_number: u32,
    missing_token: bool,
    sha_hash: Option<Vec<u8>>,
    key_values: KeyValue,
    only_public: bool,
    use_http: bool,
    http_host: String,
}

impl PICSProductInfo {
    /// Builds the product info of an app.
    fn from_app(
        app_info: &c_msg_client_pics_product_info_response::AppInfo,
        http_host: &str,
        http_min_size: u32,
    ) -> Self {
        let sha_hash = app_info.sha.clone();
        let key_values = decode_text_key_values(app_info.buffer.as_deref());

        let has_valid_http_uri =
            sha_hash.as_ref().is_some_and(|sha| !sha.is_empty()) && !http_host.is_empty();

        Self {
            id: app_info.appid.unwrap_or_default(),
            change_number: app_info.change_number.unwrap_or_default(),
            missing_token: app_info.missing_token.unwrap_or_default(),
            sha_hash,
            key_values,
            only_public: app_info.only_public.unwrap_or_default(),
            use_http: has_valid_http_uri && app_info.size.unwrap_or_default() >= http_min_size,
            http_host: http_host.to_string(),
        }
    }

    /// Builds the product info of a package.
    fn from_package(package_info: &c_msg_client_pics_product_info_response::PackageInfo) -> Self {
        let key_values = decode_binary_key_values(package_info.buffer.as_deref());

        Self {
            id: package_info.packageid.unwrap_or_default(),
            change_number: package_info.change_number.unwrap_or_default(),
            missing_token: package_info.missing_token.unwrap_or_default(),
            sha_hash: package_info.sha.clone(),
            key_values,
            only_public: false,
            use_http: false,
            http_host: String::new(),
        }
    }

    /// The id of the app or package.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// The current change number for the app or package.
    pub fn change_number(&self) -> u32 {
        self.change_number
    }

    /// Whether an access token was required for the request.
    pub fn missing_token(&self) -> bool {
        self.missing_token
    }

    /// The hash of the content.
    pub fn sha_hash(&self) -> Option<&[u8]> {
        self.sha_hash.as_deref()
    }

    /// The key value info.
    pub fn key_values(&self) -> &KeyValue {
        &self.key_values
    }

    /// For an app request, gets whether only the public information was requested.
    pub fn only_public(&self) -> bool {
        self.only_public
    }

    /// Whether HTTP should be used to load the key values data.
    pub fn use_http(&self) -> bool {
        self.use_http
    }

    /// For an app metadata-only request, gets the URI for the HTTP appinfo request.
    pub fn http_uri(&self) -> Option<String> {
        let sha_hash = self.sha_hash.as_ref()?;
        if sha_hash.is_empty() || self.http_host.is_empty() {
            return None;
        }

        Some(format!(
            "http://{}/appinfo/{}/sha/{}.txt.gz",
            self.http_host,
            self.id,
            hex::encode(sha_hash)
        ))
    }
}

/// Decodes the text key values of an app info buffer.
fn decode_text_key_values(buffer: Option<&[u8]>) -> KeyValue {
    let mut key_value = KeyValue::new();

    let Some(buffer) = buffer else {
        return key_value;
    };

    if buffer.is_empty() {
        return key_value;
    }

    // The trailing null byte isn't part of the text.
    let text = &buffer[..buffer.len() - 1];
    let mut cursor = Cursor::new(text);

    let _ = key_value.read_as_text(&mut cursor);

    key_value
}

/// Decodes the binary key values of a package info buffer.
fn decode_binary_key_values(buffer: Option<&[u8]>) -> KeyValue {
    let mut key_value = KeyValue::new();

    let Some(buffer) = buffer else {
        return key_value;
    };

    // The first four bytes are a flag the Steam client checks before reading
    // the key values.
    let payload = &buffer[buffer.len().min(4)..];
    let mut cursor = Cursor::new(payload);

    let _ = key_value.try_read_as_binary(&mut cursor);

    key_value
}

/// This callback is received when the list of guest passes is updated.
#[derive(Clone, Debug)]
pub struct GuestPassListCallback {
    base: CallbackMsgBase,
    result: EResult,
    count_guest_passes_to_give: i32,
    count_guest_passes_to_redeem: i32,
    guest_passes: Vec<KeyValue>,
}

impl GuestPassListCallback {
    /// Builds the callback from a received `ClientUpdateGuestPassesList` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let mut guest_passes =
            ClientMsg::<MsgClientUpdateGuestPassesList>::from_packet(packet_msg)?;

        let result = guest_passes.body().result;
        let count_guest_passes_to_give = guest_passes.body().count_guest_passes_to_give;
        let count_guest_passes_to_redeem = guest_passes.body().count_guest_passes_to_redeem;

        let total = (count_guest_passes_to_give + count_guest_passes_to_redeem).max(0) as usize;

        let mut passes = Vec::with_capacity(total);
        let mut reader = PayloadReader {
            payload: &mut guest_passes.payload,
        };

        for _ in 0..total {
            let mut key_value = KeyValue::new();
            if !key_value.try_read_as_binary(&mut reader) {
                break;
            }
            passes.push(key_value);
        }

        Ok(Self {
            base: CallbackMsgBase::new(),
            result,
            count_guest_passes_to_give,
            count_guest_passes_to_redeem,
            guest_passes: passes,
        })
    }

    /// The result of the operation.
    pub fn result(&self) -> EResult {
        self.result
    }

    /// The number of guest passes to be given out.
    pub fn count_guest_passes_to_give(&self) -> i32 {
        self.count_guest_passes_to_give
    }

    /// The number of guest passes to be redeemed.
    pub fn count_guest_passes_to_redeem(&self) -> i32 {
        self.count_guest_passes_to_redeem
    }

    /// The guest pass list.
    pub fn guest_passes(&self) -> &[KeyValue] {
        &self.guest_passes
    }
}

impl CallbackMsg for GuestPassListCallback {
    fn job_id(&self) -> JobID {
        self.base.job_id()
    }

    fn set_job_id(&mut self, id: JobID) {
        self.base.set_job_id(id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// This callback is received in response to activating a guest pass or a gift.
#[derive(Clone, Debug)]
pub struct RedeemGuestPassResponseCallback {
    base: CallbackMsgBase,
    result: EResult,
    package_id: u32,
    must_own_app_id: u32,
}

impl RedeemGuestPassResponseCallback {
    /// Builds the callback from a received `ClientRedeemGuestPassResponse` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let response =
            ClientMsgProtobuf::<CMsgClientRedeemGuestPassResponse>::from_packet(packet_msg)?;
        let body = response.body();

        Ok(Self {
            base: CallbackMsgBase::with_job_id(response.target_job_id()),
            result: eresult_from_u32(body.eresult),
            package_id: body.package_id.unwrap_or_default(),
            must_own_app_id: body.must_own_appid.unwrap_or_default(),
        })
    }

    /// The result of the operation.
    pub fn result(&self) -> EResult {
        self.result
    }

    /// The package id which was activated.
    pub fn package_id(&self) -> u32 {
        self.package_id
    }

    /// The app id which must be owned to activate this guest pass.
    pub fn must_own_app_id(&self) -> u32 {
        self.must_own_app_id
    }
}

impl CallbackMsg for RedeemGuestPassResponseCallback {
    fn job_id(&self) -> JobID {
        self.base.job_id()
    }

    fn set_job_id(&mut self, id: JobID) {
        self.base.set_job_id(id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// This callback is received in a response to activating a Steam key.
#[derive(Clone, Debug)]
pub struct PurchaseResponseCallback {
    base: CallbackMsgBase,
    result: EResult,
    purchase_result_detail: EPurchaseResultDetail,
    purchase_receipt_info: KeyValue,
}

impl PurchaseResponseCallback {
    /// Builds the callback from a received `ClientPurchaseResponse` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let response = ClientMsgProtobuf::<CMsgClientPurchaseResponse>::from_packet(packet_msg)?;
        let body = response.body();

        let purchase_result_detail =
            EPurchaseResultDetail::from_value(body.purchase_result_details.unwrap_or_default())
                .unwrap_or(EPurchaseResultDetail::NoDetail);

        let mut purchase_receipt_info = KeyValue::new();
        if let Some(receipt) = body.purchase_receipt_info.as_deref() {
            let mut cursor = Cursor::new(receipt);
            let _ = purchase_receipt_info.try_read_as_binary(&mut cursor);
        }

        Ok(Self {
            base: CallbackMsgBase::with_job_id(response.target_job_id()),
            result: eresult_from_i32(body.eresult),
            purchase_result_detail,
            purchase_receipt_info,
        })
    }

    /// The result of the operation.
    pub fn result(&self) -> EResult {
        self.result
    }

    /// The purchase result of the operation.
    pub fn purchase_result_detail(&self) -> EPurchaseResultDetail {
        self.purchase_result_detail
    }

    /// The purchase receipt of the operation.
    pub fn purchase_receipt_info(&self) -> &KeyValue {
        &self.purchase_receipt_info
    }
}

impl CallbackMsg for PurchaseResponseCallback {
    fn job_id(&self) -> JobID {
        self.base.job_id()
    }

    fn set_job_id(&mut self, id: JobID) {
        self.base.set_job_id(id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// This callback is received when a beta password check has been completed.
#[derive(Clone, Debug)]
pub struct CheckAppBetaPasswordCallback {
    base: CallbackMsgBase,
    result: EResult,
    beta_passwords: BTreeMap<String, Vec<u8>>,
}

impl CheckAppBetaPasswordCallback {
    /// Builds the callback from a received `ClientCheckAppBetaPasswordResponse` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let response =
            ClientMsgProtobuf::<CMsgClientCheckAppBetaPasswordResponse>::from_packet(packet_msg)?;
        let body = response.body();

        let mut beta_passwords = BTreeMap::new();
        for password in &body.betapasswords {
            let Some(name) = password.betaname.as_deref() else {
                continue;
            };

            match crate::util::utils::decode_hex_string(password.betapassword.as_deref()) {
                Ok(Some(decoded)) => {
                    beta_passwords.insert(name.to_string(), decoded);
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!("failed to decode the beta password of '{name}': {error}");
                }
            }
        }

        Ok(Self {
            base: CallbackMsgBase::with_job_id(response.target_job_id()),
            result: eresult_from_i32(body.eresult),
            beta_passwords,
        })
    }

    /// The result of the operation.
    pub fn result(&self) -> EResult {
        self.result
    }

    /// The map of beta names to their encryption keys.
    pub fn beta_passwords(&self) -> &BTreeMap<String, Vec<u8>> {
        &self.beta_passwords
    }
}

impl CallbackMsg for CheckAppBetaPasswordCallback {
    fn job_id(&self) -> JobID {
        self.base.job_id()
    }

    fn set_job_id(&mut self, id: JobID) {
        self.base.set_job_id(id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// This callback is received when a private beta request has been completed.
#[derive(Clone, Debug)]
pub struct PrivateBetaCallback {
    base: CallbackMsgBase,
    result: EResult,
    depot_section: KeyValue,
}

impl PrivateBetaCallback {
    /// Builds the callback from a received `ClientPICSPrivateBetaResponse` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let response =
            ClientMsgProtobuf::<CMsgClientPicsPrivateBetaResponse>::from_packet(packet_msg)?;
        let body = response.body();

        let depot_section = match body.depot_section.as_deref() {
            // The trailing null byte isn't part of the text.
            Some(section) if !section.is_empty() => {
                let text = &section[..section.len() - 1];
                let mut cursor = Cursor::new(text);
                let mut key_value = KeyValue::new();
                let _ = key_value.read_as_text(&mut cursor);
                key_value
            }
            _ => KeyValue::new(),
        };

        Ok(Self {
            base: CallbackMsgBase::with_job_id(response.target_job_id()),
            result: eresult_from_i32(body.eresult),
            depot_section,
        })
    }

    /// The result of the operation.
    pub fn result(&self) -> EResult {
        self.result
    }

    /// The key value info to be merged into the main appinfo.
    pub fn depot_section(&self) -> &KeyValue {
        &self.depot_section
    }
}

impl CallbackMsg for PrivateBetaCallback {
    fn job_id(&self) -> JobID {
        self.base.job_id()
    }

    fn set_job_id(&mut self, id: JobID) {
        self.base.set_job_id(id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
