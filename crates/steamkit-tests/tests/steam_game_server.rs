//! `SteamGameServer` tests. The handler posts the same `LoggedOnCallback` type
//! as `SteamUser`.

use std::sync::Arc;

use steamkit::base::generated::steam_language::EResult;
use steamkit::steam::handlers::steam_game_server::steam_game_server::{LogOnDetails, SteamGameServer};
use steamkit::steam::handlers::steam_user::callbacks::LoggedOnCallback;
use steamkit::steam::steam_client::steam_client::SteamClient;

/// A client plus its `SteamGameServer` handler.
fn handler_and_client() -> (Arc<SteamClient>, Arc<SteamGameServer>) {
    let client = SteamClient::new();
    let handler = client
        .get_handler::<SteamGameServer>()
        .expect("SteamGameServer is registered by default");
    (client, handler)
}

#[test]
fn log_on_posts_logged_on_callback_when_no_connection() {
    let (client, handler) = handler_and_client();

    let details = LogOnDetails {
        token: Some("SuperSecretToken".to_string()),
        ..Default::default()
    };

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
fn log_on_anonymous_posts_logged_on_callback_when_no_connection() {
    let (client, handler) = handler_and_client();

    // An anonymous logon without a specific app uses app id 0.
    assert!(
        handler.log_on_anonymous(0).is_ok(),
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
