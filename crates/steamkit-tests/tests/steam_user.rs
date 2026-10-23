//! `SteamUser` tests.

use std::sync::Arc;

use steamkit::base::generated::steam_language::EResult;
use steamkit::error::SteamError;
use steamkit::steam::handlers::steam_user::callbacks::LoggedOnCallback;
use steamkit::steam::handlers::steam_user::steam_user::{LogOnDetails, SteamUser};
use steamkit::steam::steam_client::steam_client::SteamClient;

/// A client plus its `SteamUser` handler.
fn handler_and_client() -> (Arc<SteamClient>, Arc<SteamUser>) {
    let client = SteamClient::new();
    let handler = client
        .get_handler::<SteamUser>()
        .expect("SteamUser is registered by default");
    (client, handler)
}

#[test]
fn log_on_posts_logged_on_callback_when_no_connection() {
    let (client, handler) = handler_and_client();

    let mut details = LogOnDetails::default();
    details.username = Some("iamauser".to_string());
    details.password = Some("lamepassword".to_string());

    assert!(
        handler.log_on(&details).is_ok(),
        "log_on posts a LoggedOnCallback and returns Ok when not connected"
    );

    let callback = client
        .get_callback()
        .expect("a LoggedOnCallback should have been posted");
    let logged_on = callback
        .as_any()
        .downcast_ref::<LoggedOnCallback>()
        .expect("the callback is a LoggedOnCallback");
    assert_eq!(logged_on.result(), EResult::NoConnection);
}

#[test]
fn log_on_throws_exception_if_details_not_provided() {
    // `LogOnDetails` with no credentials at all (no username, password or
    // access token) is rejected.
    let (_client, handler) = handler_and_client();

    let details = LogOnDetails::default();
    assert!(
        details.username.is_none()
            && details.password.is_none()
            && details.access_token.is_none(),
        "the default details must not carry any credentials"
    );

    let result = handler.log_on(&details);
    assert!(
        matches!(result, Err(SteamError::InvalidArgument(_))),
        "details without any credentials must be rejected with InvalidArgument, got {result:?}"
    );
}

#[test]
fn log_on_throws_exception_if_username_not_provided_only_password() {
    let (_client, handler) = handler_and_client();

    let mut details = LogOnDetails::default();
    details.password = Some("def".to_string());

    let result = handler.log_on(&details);
    assert!(
        matches!(result, Err(SteamError::InvalidArgument(_))),
        "a missing username must be rejected with InvalidArgument, got {result:?}"
    );
}

#[test]
fn log_on_throws_exception_if_username_not_provided_only_access_token() {
    let (_client, handler) = handler_and_client();

    let mut details = LogOnDetails::default();
    details.access_token = Some("def".to_string());

    let result = handler.log_on(&details);
    assert!(
        matches!(result, Err(SteamError::InvalidArgument(_))),
        "a missing username must be rejected with InvalidArgument, got {result:?}"
    );
}

#[test]
fn log_on_throws_exception_if_password_and_access_token_not_provided() {
    let (_client, handler) = handler_and_client();

    let mut details = LogOnDetails::default();
    details.username = Some("abc".to_string());

    let result = handler.log_on(&details);
    assert!(
        matches!(result, Err(SteamError::InvalidArgument(_))),
        "a missing password and access token must be rejected with InvalidArgument, got {result:?}"
    );
}

#[test]
fn log_on_does_not_throw_exception_if_user_name_and_password_provided() {
    let (_client, handler) = handler_and_client();

    let mut details = LogOnDetails::default();
    details.username = Some("abc".to_string());
    details.password = Some("def".to_string());

    assert!(
        handler.log_on(&details).is_ok(),
        "log_on with a username and password must not error"
    );
}

#[test]
fn log_on_does_not_throw_exception_if_user_name_and_access_token_provided() {
    let (_client, handler) = handler_and_client();

    let mut details = LogOnDetails::default();
    details.username = Some("abc".to_string());
    details.access_token = Some("def".to_string());
    details.should_remember_password = true;

    assert!(
        handler.log_on(&details).is_ok(),
        "log_on with a username and access token must not error"
    );
}

#[test]
fn log_on_anonymous_posts_logged_on_callback_when_no_connection() {
    let (client, handler) = handler_and_client();

    assert!(
        handler.log_on_anonymous().is_ok(),
        "log_on_anonymous posts a LoggedOnCallback and returns Ok when not connected"
    );

    let callback = client
        .get_callback()
        .expect("a LoggedOnCallback should have been posted");
    let logged_on = callback
        .as_any()
        .downcast_ref::<LoggedOnCallback>()
        .expect("the callback is a LoggedOnCallback");
    assert_eq!(logged_on.result(), EResult::NoConnection);
}
