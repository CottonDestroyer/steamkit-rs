//! The result of a successful authentication poll.

use steamkit_protos::steam::CAuthenticationPollAuthSessionStatusResponse;

/// Authentication poll result.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AuthPollResult {
    /// Account name of the authenticating account.
    pub account_name: String,

    /// New refresh token. Use it as `LogOnDetails::access_token`.
    pub refresh_token: String,

    /// New token subordinate to `refresh_token`.
    pub access_token: String,

    /// A remembered machine id for future logins, usually when the account uses
    /// email Steam Guard. Pass it as `AuthSessionDetails::guard_data` next time
    /// to avoid another email; store it per account.
    pub new_guard_data: Option<String>,
}

impl AuthPollResult {
    /// Creates a poll result from the response of a poll request.
    pub fn from_response(response: &CAuthenticationPollAuthSessionStatusResponse) -> Self {
        AuthPollResult {
            account_name: response.account_name.clone().unwrap_or_default(),
            refresh_token: response.refresh_token.clone().unwrap_or_default(),
            access_token: response.access_token.clone().unwrap_or_default(),
            new_guard_data: response.new_guard_data.clone(),
        }
    }
}
