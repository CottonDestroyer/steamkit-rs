//! Interacting with apps and packages: licenses, PICS, depot keys and so on.
//!
//! Requests are `async fn`s returning the response callback.
//! [`SteamApps::pics_get_product_info`] can get several responses, so it
//! returns a [`ResultSet`].

use std::sync::{Arc, Weak};

use async_trait::async_trait;
use steamkit_protos::steam::{
    c_msg_client_pics_product_info_request, CMsgClientCheckAppBetaPassword,
    CMsgClientGetAppOwnershipTicket, CMsgClientGetDepotDecryptionKey,
    CMsgClientPicsAccessTokenRequest, CMsgClientPicsChangesSinceRequest,
    CMsgClientPicsPrivateBetaRequest, CMsgClientPicsProductInfoRequest,
    CMsgClientRequestFreeLicense,
};

use crate::base::client_msg::{ClientMsg, ClientMsgProtobuf, IClientMsg};
use crate::base::generated::steam_language::internal::MsgClientGetLegacyGameKey;
use crate::base::generated::steam_language::EMsg;
use crate::base::packet_base::IPacketMsg;
use crate::error::{SteamError, SteamResult};
use crate::steam::handlers::client_msg_handler::ClientMsgHandler;
use crate::steam::steam_client::callback_mgr::callback_msg::CallbackMsg;
use crate::steam::steam_client::steam_client::SteamClient;
use crate::types::job_id::ResultSet;

use super::callbacks::{
    AppOwnershipTicketCallback, CheckAppBetaPasswordCallback, DepotKeyCallback,
    FreeLicenseCallback, GameConnectTokensCallback, GuestPassListCallback, LegacyGameKeyCallback,
    LicenseListCallback, PICSChangesCallback, PICSProductInfoCallback, PICSTokensCallback,
    PrivateBetaCallback, PurchaseResponseCallback, RedeemGuestPassResponseCallback,
    VACStatusCallback,
};

/// A PICS request, for [`SteamApps::pics_get_product_info`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PICSRequest {
    id: u32,
    access_token: u64,
}

impl PICSRequest {
    /// Creates a product info request for an app or package id and its access token.
    pub fn new(id: u32, access_token: u64) -> Self {
        Self { id, access_token }
    }

    /// The id of the app or package being requested.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Sets the id of the app or package being requested.
    pub fn set_id(&mut self, id: u32) {
        self.id = id;
    }

    /// The access token associated with the request.
    pub fn access_token(&self) -> u64 {
        self.access_token
    }

    /// Sets the access token associated with the request.
    pub fn set_access_token(&mut self, access_token: u64) {
        self.access_token = access_token;
    }
}

/// Interacts with apps and packages on the Steam network.
pub struct SteamApps {
    client: Weak<SteamClient>,
}

impl std::fmt::Debug for SteamApps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SteamApps").finish()
    }
}

impl SteamApps {
    /// Creates the handler and stores a weak back-reference to the client.
    pub fn new(client: &Arc<SteamClient>) -> Self {
        Self {
            client: Arc::downgrade(client),
        }
    }

    /// Upgrades the weak client reference.
    fn client(&self) -> SteamResult<Arc<SteamClient>> {
        self.client
            .upgrade()
            .ok_or_else(|| SteamError::Connection("SteamClient has been dropped".to_string()))
    }

    /// Requests an app ownership ticket for the specified app id.
    ///
    /// Results are returned in an [`AppOwnershipTicketCallback`].
    pub async fn get_app_ownership_ticket(
        &self,
        appid: u32,
    ) -> SteamResult<AppOwnershipTicketCallback> {
        let client = self.client()?;

        let mut request = ClientMsgProtobuf::<CMsgClientGetAppOwnershipTicket>::new(
            EMsg::ClientGetAppOwnershipTicket,
        );

        request.body_mut().app_id = Some(appid);

        let job_id = client.get_next_job_id();
        request.set_source_job_id(job_id);

        let job = client.start_job::<AppOwnershipTicketCallback>(job_id);

        client.send(&request)?;

        job.wait().await
    }

    /// Requests the depot decryption key for a specified depot id.
    ///
    /// Results are returned in a [`DepotKeyCallback`].
    pub async fn get_depot_decryption_key(
        &self,
        depot_id: u32,
        app_id: u32,
    ) -> SteamResult<DepotKeyCallback> {
        let client = self.client()?;

        let mut request = ClientMsgProtobuf::<CMsgClientGetDepotDecryptionKey>::new(
            EMsg::ClientGetDepotDecryptionKey,
        );

        request.body_mut().depot_id = Some(depot_id);
        request.body_mut().app_id = Some(app_id);

        let job_id = client.get_next_job_id();
        request.set_source_job_id(job_id);

        let job = client.start_job::<DepotKeyCallback>(job_id);

        client.send(&request)?;

        job.wait().await
    }

    /// Requests PICS access tokens for an app or a package.
    ///
    /// Results are returned in a [`PICSTokensCallback`].
    pub async fn pics_get_access_tokens(
        &self,
        app: Option<u32>,
        package: Option<u32>,
    ) -> SteamResult<PICSTokensCallback> {
        let apps: Vec<u32> = app.into_iter().collect();
        let packages: Vec<u32> = package.into_iter().collect();

        self.pics_get_access_tokens_for_ids(&apps, &packages).await
    }

    /// Requests PICS access tokens for a list of app ids and package ids.
    ///
    /// Results are returned in a [`PICSTokensCallback`].
    pub async fn pics_get_access_tokens_for_ids(
        &self,
        app_ids: &[u32],
        package_ids: &[u32],
    ) -> SteamResult<PICSTokensCallback> {
        let client = self.client()?;

        let mut request = ClientMsgProtobuf::<CMsgClientPicsAccessTokenRequest>::new(
            EMsg::ClientPICSAccessTokenRequest,
        );

        request.body_mut().appids = app_ids.to_vec();
        request.body_mut().packageids = package_ids.to_vec();

        let job_id = client.get_next_job_id();
        request.set_source_job_id(job_id);

        let job = client.start_job::<PICSTokensCallback>(job_id);

        client.send(&request)?;

        job.wait().await
    }

    /// Requests the changes for apps and packages since a given change number.
    ///
    /// Results are returned in a [`PICSChangesCallback`].
    pub async fn pics_get_changes_since(
        &self,
        last_change_number: u32,
        send_app_changelist: bool,
        send_package_changelist: bool,
    ) -> SteamResult<PICSChangesCallback> {
        let client = self.client()?;

        let mut request = ClientMsgProtobuf::<CMsgClientPicsChangesSinceRequest>::new(
            EMsg::ClientPICSChangesSinceRequest,
        );

        request.body_mut().since_change_number = Some(last_change_number);
        request.body_mut().send_app_info_changes = Some(send_app_changelist);
        request.body_mut().send_package_info_changes = Some(send_package_changelist);

        let job_id = client.get_next_job_id();
        request.set_source_job_id(job_id);

        let job = client.start_job::<PICSChangesCallback>(job_id);

        client.send(&request)?;

        job.wait().await
    }

    /// Requests the product information of an app or a package.
    ///
    /// Results are returned in a [`ResultSet`] of [`PICSProductInfoCallback`].
    pub async fn pics_get_product_info(
        &self,
        app: Option<&PICSRequest>,
        package: Option<&PICSRequest>,
        meta_data_only: bool,
    ) -> SteamResult<ResultSet<PICSProductInfoCallback>> {
        let apps: Vec<PICSRequest> = app.into_iter().copied().collect();
        let packages: Vec<PICSRequest> = package.into_iter().copied().collect();

        self.pics_get_product_info_for_ids(&apps, &packages, meta_data_only)
            .await
    }

    /// Requests the product information of a list of apps or packages.
    ///
    /// Results are returned in a [`ResultSet`] of [`PICSProductInfoCallback`].
    pub async fn pics_get_product_info_for_ids(
        &self,
        apps: &[PICSRequest],
        packages: &[PICSRequest],
        meta_data_only: bool,
    ) -> SteamResult<ResultSet<PICSProductInfoCallback>> {
        let client = self.client()?;

        let mut request = ClientMsgProtobuf::<CMsgClientPicsProductInfoRequest>::new(
            EMsg::ClientPICSProductInfoRequest,
        );

        {
            let body = request.body_mut();

            for app in apps {
                body.apps
                    .push(c_msg_client_pics_product_info_request::AppInfo {
                        appid: Some(app.id()),
                        access_token: Some(app.access_token()),
                        only_public_obsolete: Some(false),
                    });
            }

            for package in packages {
                body.packages
                    .push(c_msg_client_pics_product_info_request::PackageInfo {
                        packageid: Some(package.id()),
                        access_token: Some(package.access_token()),
                    });
            }

            body.meta_data_only = Some(meta_data_only);
        }

        let job_id = client.get_next_job_id();
        request.set_source_job_id(job_id);

        let handle = client
            .job_manager()
            .start_job_multiple::<PICSProductInfoCallback>(job_id, |callback| {
                !callback.response_pending()
            });

        if !client.is_connected() {
            client.job_manager().fail_job(job_id);
        }

        client.send(&request)?;

        let result = handle.wait().await?;

        Ok(result)
    }

    /// Requests a free license for the given app id.
    ///
    /// Results are returned in a [`FreeLicenseCallback`].
    pub async fn request_free_license(&self, app: u32) -> SteamResult<FreeLicenseCallback> {
        self.request_free_licenses(&[app]).await
    }

    /// Requests a free license for the given app ids.
    ///
    /// Results are returned in a [`FreeLicenseCallback`].
    pub async fn request_free_licenses(&self, apps: &[u32]) -> SteamResult<FreeLicenseCallback> {
        let client = self.client()?;

        let mut request =
            ClientMsgProtobuf::<CMsgClientRequestFreeLicense>::new(EMsg::ClientRequestFreeLicense);

        request.body_mut().appids = apps.to_vec();

        let job_id = client.get_next_job_id();
        request.set_source_job_id(job_id);

        let job = client.start_job::<FreeLicenseCallback>(job_id);

        client.send(&request)?;

        job.wait().await
    }

    /// Submits a beta password for a given app to retrieve its betas and encryption keys.
    ///
    /// Results are returned in a [`CheckAppBetaPasswordCallback`].
    pub async fn check_app_beta_password(
        &self,
        app: u32,
        password: &str,
    ) -> SteamResult<CheckAppBetaPasswordCallback> {
        let client = self.client()?;

        let mut request = ClientMsgProtobuf::<CMsgClientCheckAppBetaPassword>::new(
            EMsg::ClientCheckAppBetaPassword,
        );

        request.body_mut().app_id = Some(app);
        request.body_mut().betapassword = Some(password.to_string());

        let job_id = client.get_next_job_id();
        request.set_source_job_id(job_id);

        let job = client.start_job::<CheckAppBetaPasswordCallback>(job_id);

        client.send(&request)?;

        job.wait().await
    }

    /// Requests the legacy CD game keys of the requested app id.
    ///
    /// Results are returned in a [`LegacyGameKeyCallback`].
    pub async fn get_legacy_game_key(&self, appid: u32) -> SteamResult<LegacyGameKeyCallback> {
        let client = self.client()?;

        let mut request = ClientMsg::<MsgClientGetLegacyGameKey>::new(EMsg::ClientGetLegacyGameKey);

        request.body_mut().app_id = appid;

        let job_id = client.get_next_job_id();
        request.set_source_job_id(job_id);

        let job = client.start_job::<LegacyGameKeyCallback>(job_id);

        client.send(&request)?;

        job.wait().await
    }

    /// Requests the private beta section of a given app.
    ///
    /// Results are returned in a [`PrivateBetaCallback`].
    pub async fn pics_get_private_beta(
        &self,
        app: u32,
        access_token: u64,
        branch: &str,
        branch_password_hash: &[u8],
    ) -> SteamResult<PrivateBetaCallback> {
        let client = self.client()?;

        let mut request = ClientMsgProtobuf::<CMsgClientPicsPrivateBetaRequest>::new(
            EMsg::ClientPICSPrivateBetaRequest,
        );

        request.body_mut().appid = Some(app);
        request.body_mut().access_token = Some(access_token);
        request.body_mut().beta_name = Some(branch.to_string());
        request.body_mut().password_hash = Some(branch_password_hash.to_vec());

        let job_id = client.get_next_job_id();
        request.set_source_job_id(job_id);

        let job = client.start_job::<PrivateBetaCallback>(job_id);

        client.send(&request)?;

        job.wait().await
    }
}

#[async_trait]
impl ClientMsgHandler for SteamApps {
    async fn handle_msg(
        &self,
        client: &SteamClient,
        packet_msg: &dyn IPacketMsg,
    ) -> SteamResult<()> {
        let callback: Option<Box<dyn CallbackMsg>> = match packet_msg.msg_type() {
            EMsg::ClientLicenseList => {
                Some(Box::new(LicenseListCallback::from_packet(packet_msg)?))
            }
            EMsg::ClientRequestFreeLicenseResponse => {
                Some(Box::new(FreeLicenseCallback::from_packet(packet_msg)?))
            }
            EMsg::ClientPurchaseResponse => {
                Some(Box::new(PurchaseResponseCallback::from_packet(packet_msg)?))
            }
            EMsg::ClientRedeemGuestPassResponse => Some(Box::new(
                RedeemGuestPassResponseCallback::from_packet(packet_msg)?,
            )),
            EMsg::ClientGameConnectTokens => Some(Box::new(
                GameConnectTokensCallback::from_packet(packet_msg)?,
            )),
            EMsg::ClientVACBanStatus => Some(Box::new(VACStatusCallback::from_packet(packet_msg)?)),
            EMsg::ClientGetAppOwnershipTicketResponse => Some(Box::new(
                AppOwnershipTicketCallback::from_packet(packet_msg)?,
            )),
            EMsg::ClientGetDepotDecryptionKeyResponse => {
                Some(Box::new(DepotKeyCallback::from_packet(packet_msg)?))
            }
            EMsg::ClientGetLegacyGameKeyResponse => {
                Some(Box::new(LegacyGameKeyCallback::from_packet(packet_msg)?))
            }
            EMsg::ClientPICSAccessTokenResponse => {
                Some(Box::new(PICSTokensCallback::from_packet(packet_msg)?))
            }
            EMsg::ClientPICSChangesSinceResponse => {
                Some(Box::new(PICSChangesCallback::from_packet(packet_msg)?))
            }
            EMsg::ClientPICSProductInfoResponse => {
                Some(Box::new(PICSProductInfoCallback::from_packet(packet_msg)?))
            }
            EMsg::ClientUpdateGuestPassesList => {
                Some(Box::new(GuestPassListCallback::from_packet(packet_msg)?))
            }
            EMsg::ClientCheckAppBetaPasswordResponse => Some(Box::new(
                CheckAppBetaPasswordCallback::from_packet(packet_msg)?,
            )),
            EMsg::ClientPICSPrivateBetaResponse => {
                Some(Box::new(PrivateBetaCallback::from_packet(packet_msg)?))
            }
            // Not a message we handle.
            _ => None,
        };

        if let Some(callback) = callback {
            client.post_callback(callback);
        }

        Ok(())
    }
}
