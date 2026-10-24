//! Authenticating with Steam: the credentials and QR code flows.
//!
//! These go through the `Authentication` unified service. prost doesn't
//! generate service clients, so `SteamAuthentication::call_service` sends the
//! request itself and `AuthenticationServiceHandler` matches the response to
//! it by job id.

use std::collections::HashMap;
use std::sync::{Arc, Weak};
use std::time::Duration;

use async_trait::async_trait;
use base64::prelude::BASE64_STANDARD;
use base64::Engine;
use parking_lot::Mutex;
use rsa::pkcs1v15::Pkcs1v15Encrypt;
use rsa::{BigUint, RsaPublicKey};
use steamkit_protos::steam::{
    CAuthenticationAccessTokenGenerateForAppRequest,
    CAuthenticationAccessTokenGenerateForAppResponse,
    CAuthenticationBeginAuthSessionViaCredentialsRequest,
    CAuthenticationBeginAuthSessionViaCredentialsResponse, CAuthenticationBeginAuthSessionViaQrRequest,
    CAuthenticationBeginAuthSessionViaQrResponse, CAuthenticationDeviceDetails,
    CAuthenticationGetPasswordRsaPublicKeyRequest,
    CAuthenticationGetPasswordRsaPublicKeyResponse, CAuthenticationPollAuthSessionStatusRequest,
    CAuthenticationPollAuthSessionStatusResponse,
    CAuthenticationUpdateAuthSessionWithSteamGuardCodeRequest,
    CAuthenticationUpdateAuthSessionWithSteamGuardCodeResponse, EAuthSessionGuardType,
    ESessionPersistence, ETokenRenewalType,
};
use tokio::sync::oneshot;

use crate::base::client_msg::{ClientMsgProtobuf, IClientMsg};
use crate::base::generated::steam_language::{EMsg, EResult};
use crate::base::packet_base::{IPacketMsg, PacketClientMsgProtobuf};
use crate::error::SteamError;
use crate::steam::handlers::client_msg_handler::ClientMsgHandler;
use crate::steam::steam_client::steam_client::SteamClient;
use crate::types::job_id::JobID;
use crate::types::steam_id::SteamID;

use super::access_token_generate_result::AccessTokenGenerateResult;
use super::auth_session::{AuthSession, SessionKind};
use super::auth_session_details::AuthSessionDetails;
use super::authentication_exception::AuthenticationException;
use super::credentials_auth_session::CredentialsAuthSession;
use super::qr_auth_session::{QrAuthSession, QrAuthSessionState};

/// The unified service that handles authentication.
const AUTHENTICATION_SERVICE: &str = "Authentication";

/// The version of the service methods we call.
const AUTHENTICATION_SERVICE_VERSION: u32 = 1;

/// How long a service call waits for its response (the usual 10 second job
/// timeout). It has to give up at some point: if the CM connection is replaced
/// while a request is in flight, the answer never arrives.
const SERVICE_METHOD_TIMEOUT: Duration = Duration::from_secs(10);

/// The raw response of a unified service call.
struct RawServiceResponse {
    result: EResult,
    body: Vec<u8>,
}

/// Unified service calls waiting for their response.
#[derive(Clone, Default)]
struct PendingRequests {
    requests: Arc<Mutex<HashMap<JobID, oneshot::Sender<RawServiceResponse>>>>,
}

impl PendingRequests {
    /// Registers a call that's waiting for its response.
    fn insert(&self, job_id: JobID, sender: oneshot::Sender<RawServiceResponse>) {
        self.requests.lock().insert(job_id, sender);
    }

    /// Removes a call that won't be answered anymore.
    fn remove(&self, job_id: JobID) {
        self.requests.lock().remove(&job_id);
    }

    /// Hands a response to the caller waiting for it.
    fn complete(&self, job_id: JobID, response: RawServiceResponse) {
        let sender = self.requests.lock().remove(&job_id);

        if let Some(sender) = sender {
            // Fails only if the caller already gave up; nothing to do then.
            let _ = sender.send(response);
        }
    }
}

/// Receives the responses of the authentication service.
struct AuthenticationServiceHandler {
    pending: PendingRequests,
}

#[async_trait]
impl ClientMsgHandler for AuthenticationServiceHandler {
    /// Hands the response of a service call to the waiting caller.
    async fn handle_msg(
        &self,
        _client: &SteamClient,
        packet: &dyn IPacketMsg,
    ) -> crate::error::SteamResult<()> {
        if packet.msg_type() != EMsg::ServiceMethodResponse {
            return Ok(());
        }

        if !packet.is_proto() {
            return Ok(());
        }

        let Ok(proto_packet) =
            PacketClientMsgProtobuf::new(packet.msg_type(), packet.get_data().to_vec())
        else {
            return Ok(());
        };

        let header = proto_packet.header();

        let job_id = JobID::from_value(header.proto.jobid_target.unwrap_or(u64::MAX));
        let result = header
            .proto
            .eresult
            .and_then(EResult::from_value)
            .unwrap_or(EResult::Invalid);

        let data = proto_packet.get_data();
        let offset = proto_packet.body_offset() as usize;
        let body = data
            .get(offset..)
            .map(|bytes| bytes.to_vec())
            .unwrap_or_default();

        self.pending
            .complete(job_id, RawServiceResponse { result, body });

        Ok(())
    }
}

/// Authenticates with Steam.
pub struct SteamAuthentication {
    client: Weak<SteamClient>,
    pending: PendingRequests,
}

impl std::fmt::Debug for SteamAuthentication {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SteamAuthentication").finish()
    }
}

impl SteamAuthentication {
    /// Creates the handler and registers the response handler with the client.
    pub fn new(client: Weak<SteamClient>) -> Self {
        let pending = PendingRequests::default();

        if let Some(client) = client.upgrade() {
            client.add_handler(Arc::new(AuthenticationServiceHandler {
                pending: pending.clone(),
            }));
        }

        SteamAuthentication { client, pending }
    }

    /// Gets the RSA public key for an account name, used to encrypt the
    /// password.
    pub(crate) async fn get_password_rsa_public_key(
        &self,
        account_name: &str,
    ) -> Result<CAuthenticationGetPasswordRsaPublicKeyResponse, AuthenticationException> {
        let request = CAuthenticationGetPasswordRsaPublicKeyRequest {
            account_name: Some(account_name.to_string()),
        };

        let (result, body) = self.call_service("GetPasswordRSAPublicKey", request).await?;

        if result != EResult::OK {
            return Err(AuthenticationException::failed(
                "Failed to get password public key",
                result,
            ));
        }

        Ok(body)
    }

    /// Generates an access token from a refresh token for a client app audience
    /// (desktop or mobile client). `allow_renewal` lets Steam renew the refresh
    /// token too.
    pub async fn generate_access_token_for_app(
        &self,
        steam_id: SteamID,
        refresh_token: &str,
        allow_renewal: bool,
    ) -> Result<AccessTokenGenerateResult, SteamError> {
        let mut request = CAuthenticationAccessTokenGenerateForAppRequest {
            refresh_token: Some(refresh_token.to_string()),
            steamid: Some(steam_id.convert_to_u64()),
            renewal_type: None,
        };

        if allow_renewal {
            request.renewal_type = Some(ETokenRenewalType::KETokenRenewalTypeAllow as i32);
        }

        let (result, body): (EResult, CAuthenticationAccessTokenGenerateForAppResponse) = self
            .call_service("GenerateAccessTokenForApp", request)
            .await
            .map_err(SteamError::from)?;

        if result != EResult::OK {
            return Err(
                AuthenticationException::failed("Failed to generate token", result).into(),
            );
        }

        Ok(AccessTokenGenerateResult::from_response(&body))
    }

    /// Starts authenticating with a QR code.
    pub async fn begin_auth_session_via_qr(
        &self,
        details: &AuthSessionDetails,
    ) -> Result<QrAuthSession, SteamError> {
        let client = self.client.upgrade().ok_or_else(|| {
            SteamError::Connection("The SteamClient instance has been dropped.".to_string())
        })?;

        if !client.is_connected() {
            return Err(SteamError::InvalidArgument(
                "The SteamClient instance must be connected.".to_string(),
            ));
        }

        let request = CAuthenticationBeginAuthSessionViaQrRequest {
            website_id: details.website_id.clone(),
            device_details: Some(device_details(details)),
            ..Default::default()
        };

        let (result, body): (EResult, CAuthenticationBeginAuthSessionViaQrResponse) = self
            .call_service("BeginAuthSessionViaQR", request)
            .await
            .map_err(SteamError::from)?;

        if result != EResult::OK {
            return Err(
                AuthenticationException::failed("Failed to begin QR auth session", result).into(),
            );
        }

        let state = Arc::new(QrAuthSessionState::new(
            body.challenge_url.clone().unwrap_or_default(),
        ));

        let session = AuthSession::new(
            Arc::downgrade(&client),
            details.authenticator.clone(),
            body.client_id.unwrap_or_default(),
            body.request_id.clone().unwrap_or_default(),
            body.allowed_confirmations.clone(),
            body.interval.unwrap_or_default(),
            SessionKind::Qr { state: state.clone() },
        );

        Ok(QrAuthSession::new(session, state))
    }

    /// Starts authenticating with a username and password.
    pub async fn begin_auth_session_via_credentials(
        &self,
        details: &AuthSessionDetails,
    ) -> Result<CredentialsAuthSession, SteamError> {
        let client = self.client.upgrade().ok_or_else(|| {
            SteamError::Connection("The SteamClient instance has been dropped.".to_string())
        })?;

        let (Some(username), Some(password)) = (&details.username, &details.password) else {
            return Err(SteamError::InvalidArgument(
                "BeginAuthSessionViaCredentials requires a username and password to be set in 'details'.".to_string(),
            ));
        };

        if username.is_empty() || password.is_empty() {
            return Err(SteamError::InvalidArgument(
                "BeginAuthSessionViaCredentials requires a username and password to be set in 'details'.".to_string(),
            ));
        }

        if !client.is_connected() {
            return Err(SteamError::InvalidArgument(
                "The SteamClient instance must be connected.".to_string(),
            ));
        }

        // Encrypt the password.
        let public_key = self
            .get_password_rsa_public_key(username)
            .await
            .map_err(SteamError::from)?;

        let encrypted_password = encrypt_password(&public_key, password).map_err(SteamError::from)?;

        let request = CAuthenticationBeginAuthSessionViaCredentialsRequest {
            account_name: Some(username.clone()),
            persistence: Some(if details.is_persistent_session {
                ESessionPersistence::KESessionPersistencePersistent as i32
            } else {
                ESessionPersistence::KESessionPersistenceEphemeral as i32
            }),
            website_id: details.website_id.clone(),
            guard_data: details.guard_data.clone(),
            encrypted_password: Some(BASE64_STANDARD.encode(&encrypted_password)),
            encryption_timestamp: public_key.timestamp,
            device_details: Some(device_details(details)),
            ..Default::default()
        };

        let (result, body): (EResult, CAuthenticationBeginAuthSessionViaCredentialsResponse) = self
            .call_service("BeginAuthSessionViaCredentials", request)
            .await
            .map_err(SteamError::from)?;

        // InvalidPassword, ServiceUnavailable, InvalidParam or RateLimitExceeded.
        if result != EResult::OK {
            return Err(AuthenticationException::failed("Authentication failed", result).into());
        }

        let session = AuthSession::new(
            Arc::downgrade(&client),
            details.authenticator.clone(),
            body.client_id.unwrap_or_default(),
            body.request_id.clone().unwrap_or_default(),
            body.allowed_confirmations.clone(),
            body.interval.unwrap_or_default(),
            SessionKind::Credentials {
                steam_id: SteamID::from_id(body.steamid.unwrap_or_default()),
            },
        );

        Ok(CredentialsAuthSession::new(session))
    }

    /// Polls the status of an authentication session.
    pub(crate) async fn poll_auth_session_status(
        &self,
        client_id: u64,
        request_id: Vec<u8>,
    ) -> Result<CAuthenticationPollAuthSessionStatusResponse, AuthenticationException> {
        let request = CAuthenticationPollAuthSessionStatusRequest {
            client_id: Some(client_id),
            request_id: Some(request_id),
            ..Default::default()
        };

        let (result, body) = self.call_service("PollAuthSessionStatus", request).await?;

        // Expired, FileNotFound or Fail.
        if result != EResult::OK {
            return Err(AuthenticationException::failed("Failed to poll status", result));
        }

        Ok(body)
    }

    /// Sends a Steam Guard code for an authentication session.
    pub(crate) async fn send_steam_guard_code(
        &self,
        client_id: u64,
        steam_id: SteamID,
        code: &str,
        code_type: EAuthSessionGuardType,
    ) -> Result<(), AuthenticationException> {
        let request = CAuthenticationUpdateAuthSessionWithSteamGuardCodeRequest {
            client_id: Some(client_id),
            steamid: Some(steam_id.convert_to_u64()),
            code: Some(code.to_string()),
            code_type: Some(code_type as i32),
        };

        let (result, _body): (EResult, CAuthenticationUpdateAuthSessionWithSteamGuardCodeResponse) = self
            .call_service("UpdateAuthSessionWithSteamGuardCode", request)
            .await?;

        // Seen: InvalidLoginAuthCode, TwoFactorCodeMismatch, Expired and
        // DuplicateRequest. DuplicateRequest happens when the prompt was already
        // accepted in the mobile app; the next poll succeeds, so it's not an error.
        if result != EResult::OK && result != EResult::DuplicateRequest {
            return Err(AuthenticationException::failed(
                "Failed to send steam guard code",
                result,
            ));
        }

        Ok(())
    }

    /// Calls a method of the `Authentication` unified service.
    async fn call_service<Req, Resp>(
        &self,
        method: &str,
        request: Req,
    ) -> Result<(EResult, Resp), AuthenticationException>
    where
        Req: prost::Message + Default,
        Resp: prost::Message + Default,
    {
        let client = self.client.upgrade().ok_or_else(|| {
            AuthenticationException::other("The SteamClient instance has been dropped.")
        })?;

        let job_id = client.get_next_job_id();
        let (sender, receiver) = oneshot::channel();
        self.pending.insert(job_id, sender);

        let e_msg = if client.steam_id().is_some() {
            EMsg::ServiceMethodCallFromClient
        } else {
            EMsg::ServiceMethodCallFromClientNonAuthed
        };

        let mut msg = ClientMsgProtobuf::<Req>::with_payload_reserve(e_msg, 64);
        msg.set_source_job_id(job_id);
        msg.header_mut().proto.target_job_name = Some(format!(
            "{AUTHENTICATION_SERVICE}.{method}#{AUTHENTICATION_SERVICE_VERSION}"
        ));
        msg.set_body(request);

        if let Err(error) = client.send(&msg) {
            self.pending.remove(job_id);

            return Err(AuthenticationException::other(format!(
                "Failed to send the authentication request: {error}"
            )));
        }

        let response = match tokio::time::timeout(SERVICE_METHOD_TIMEOUT, receiver).await {
            Ok(Ok(response)) => response,
            Ok(Err(_)) => {
                self.pending.remove(job_id);

                return Err(AuthenticationException::other(
                    "The authentication request was cancelled.",
                ));
            }
            Err(_) => {
                self.pending.remove(job_id);

                return Err(AuthenticationException::other(format!(
                    "The authentication request timed out after {} seconds.",
                    SERVICE_METHOD_TIMEOUT.as_secs()
                )));
            }
        };

        let body = Resp::decode(response.body.as_slice()).map_err(|error| {
            AuthenticationException::other(format!(
                "Failed to decode the authentication response: {error}"
            ))
        })?;

        Ok((response.result, body))
    }
}

/// The device details sent with a login request.
fn device_details(details: &AuthSessionDetails) -> CAuthenticationDeviceDetails {
    CAuthenticationDeviceDetails {
        device_friendly_name: details.device_friendly_name.clone(),
        platform_type: Some(details.platform_type as i32),
        os_type: Some(i32::from(details.client_os_type)),
        ..Default::default()
    }
}

/// Encrypts the password with the account's RSA public key (PKCS#1 v1.5).
fn encrypt_password(
    public_key: &CAuthenticationGetPasswordRsaPublicKeyResponse,
    password: &str,
) -> Result<Vec<u8>, AuthenticationException> {
    let modulus = hex::decode(public_key.publickey_mod.as_deref().unwrap_or_default()).map_err(
        |error| {
            AuthenticationException::other(format!("Failed to decode the RSA modulus: {error}"))
        },
    )?;

    let exponent = hex::decode(public_key.publickey_exp.as_deref().unwrap_or_default()).map_err(
        |error| {
            AuthenticationException::other(format!("Failed to decode the RSA exponent: {error}"))
        },
    )?;

    let key = RsaPublicKey::new(
        BigUint::from_bytes_be(&modulus),
        BigUint::from_bytes_be(&exponent),
    )
    .map_err(|error| {
        AuthenticationException::other(format!("Failed to import the RSA public key: {error}"))
    })?;

    let mut rng = rand::thread_rng();

    key.encrypt(&mut rng, Pkcs1v15Encrypt, password.as_bytes())
        .map_err(|error| {
            AuthenticationException::other(format!("Failed to encrypt the password: {error}"))
        })
}
