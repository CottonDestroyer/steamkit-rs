//! The result of generating an access token from a refresh token.

use steamkit_protos::steam::CAuthenticationAccessTokenGenerateForAppResponse;

/// Access token generation result.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AccessTokenGenerateResult {
    /// New refresh token. Can be used as `LogOnDetails::access_token` and with
    /// `SteamAuthentication::generate_access_token_for_app`. May be empty.
    pub refresh_token: String,

    /// New token subordinate to `refresh_token`.
    pub access_token: String,
}

impl AccessTokenGenerateResult {
    /// Creates a result from the response of a token generation request.
    pub fn from_response(response: &CAuthenticationAccessTokenGenerateForAppResponse) -> Self {
        AccessTokenGenerateResult {
            refresh_token: response.refresh_token.clone().unwrap_or_default(),
            access_token: response.access_token.clone().unwrap_or_default(),
        }
    }
}
