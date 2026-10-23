//! Shows the unified service API. Unified services are a type of webapi
//! service that can be accessed with either HTTP requests or through the
//! Steam network.
//!
//! This sample calls the IPlayer unified service over the Steam connection.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use steamkit::base::generated::steam_language::EResult;
use steamkit::steam::handlers::steam_unified_messages::callbacks::ServiceMethodNotification;
use steamkit::steam::handlers::steam_unified_messages::steam_unified_messages::SteamUnifiedMessages;
use steamkit::steam::handlers::steam_user::callbacks::{LoggedOffCallback, LoggedOnCallback};
use steamkit::steam::handlers::steam_user::steam_user::{LogOnDetails, SteamUser};
use steamkit::steam::steam_client::callbacks::{ConnectedCallback, DisconnectedCallback};
use steamkit::steam::steam_client::steam_client::SteamClient;
use steamkit_protos::steam::{
    CGameNotificationsOnNotificationsRequestedNotification, CPlayerGetGameBadgeLevelsRequest,
    CPlayerGetGameBadgeLevelsResponse,
};

/// The name of the unified service that is used in this sample.
const PLAYER_SERVICE: &str = "Player";

/// The name of the service that sends the game notifications.
const GAME_NOTIFICATIONS_SERVICE: &str = "GameNotificationsClient";

/// The name of the service method that is called.
const GET_GAME_BADGE_LEVELS_METHOD: &str = "GetGameBadgeLevels";

/// Queries the IPlayer service for our badge levels.
async fn on_logged_on(
    steam_unified_messages: Arc<SteamUnifiedMessages>,
    steam_user: Arc<SteamUser>,
    is_running: Arc<AtomicBool>,
) {
    println!("Successfully logged on!");

    // now that we're logged onto Steam, lets query the IPlayer service for our
    // badge levels

    // first, build our request object, these are generated from the protobuf
    // definitions and live in the steamkit-protos crate
    let request = CPlayerGetGameBadgeLevelsRequest {
        // we want to know our 440 (TF2) badge level
        appid: Some(440),
    };

    // now lets send the request and await for the response
    let player_service = match steam_unified_messages.create_service(PLAYER_SERVICE) {
        Ok(service) => service,
        Err(error) => {
            eprintln!("Unable to create the {PLAYER_SERVICE} service: {error}");
            is_running.store(false, Ordering::SeqCst);
            return;
        }
    };

    player_service.set_response_type::<CPlayerGetGameBadgeLevelsResponse>(GET_GAME_BADGE_LEVELS_METHOD);

    let _service_response = match player_service
        .send_message::<CPlayerGetGameBadgeLevelsRequest, CPlayerGetGameBadgeLevelsResponse>(
            GET_GAME_BADGE_LEVELS_METHOD,
            request.clone(),
        )
        .await
    {
        Ok(response) => response,
        Err(error) => {
            eprintln!("Unified service request failed: {error}");
            is_running.store(false, Ordering::SeqCst);
            return;
        }
    };

    // alternatively, the request can be made using SteamUnifiedMessages
    // directly, but then you must build the service request name manually. the
    // name format is in the form of <Service>.<Method>#<Version>
    let response = match steam_unified_messages
        .send_message::<CPlayerGetGameBadgeLevelsRequest, CPlayerGetGameBadgeLevelsResponse>(
            "Player.GetGameBadgeLevels#1",
            request,
        )
        .await
    {
        Ok(response) => response,
        Err(error) => {
            eprintln!("Unified service request failed: {error}");
            is_running.store(false, Ordering::SeqCst);
            return;
        }
    };

    if response.result() != EResult::OK {
        println!("Unified service request failed with {:?}", response.result());
        return;
    }

    println!(
        "Our player level is {}",
        response.body().player_level.unwrap_or_default()
    );

    for badge in &response.body().badges {
        println!(
            "Badge series {} is level {}",
            badge.series.unwrap_or_default(),
            badge.level.unwrap_or_default()
        );
    }

    // now that we've completed our task, lets log off after a few seconds to
    // receive possible notifications
    tokio::time::sleep(Duration::from_secs(10)).await;

    let _ = steam_user.log_off();
}

/// Logs on to Steam and queries the IPlayer unified service.
#[tokio::main]
async fn main() -> Result<()> {
    let mut arguments = std::env::args().skip(1);

    let (Some(user), Some(pass)) = (arguments.next(), arguments.next()) else {
        println!("Sample8: No username and password specified!");
        return Ok(());
    };

    // create our steamclient instance
    let steam_client = SteamClient::new();
    // get the steamuser handler, which is used for logging on after successfully connecting
    let steam_user = steam_client.get_required_handler::<SteamUser>()?;

    // get the steam unified messages handler, which is used for sending and
    // receiving responses from the unified service api
    let steam_unified_messages = steam_client.get_required_handler::<SteamUnifiedMessages>()?;

    let is_running = Arc::new(AtomicBool::new(true));

    // subscribe to incoming messages from the GameNotificationsClient service
    {
        let notification_service = steam_unified_messages.create_service(GAME_NOTIFICATIONS_SERVICE)?;

        notification_service.set_notification_type::<CGameNotificationsOnNotificationsRequestedNotification>(
            "OnNotificationsRequested",
        );

        steam_client.callback_manager().subscribe_service_notification::<
            ServiceMethodNotification<CGameNotificationsOnNotificationsRequestedNotification>,
        >(Box::new(move |notification| {
            println!(
                "User with id {} started the game:",
                notification.body().steamid.unwrap_or_default()
            );
            println!("{}", notification.body().appid.unwrap_or_default());
        }));
    }

    // register a few callbacks we're interested in
    {
        let steam_user = steam_user.clone();
        let user = user.clone();
        let pass = pass.clone();

        steam_client.subscribe::<ConnectedCallback>(Box::new(move |_callback| {
            println!("Connected to Steam! Logging in '{user}'...");

            let details = LogOnDetails {
                username: Some(user.clone()),
                password: Some(pass.clone()),
                ..Default::default()
            };

            let _ = steam_user.log_on(&details);
        }));
    }

    {
        let is_running = is_running.clone();

        steam_client.subscribe::<DisconnectedCallback>(Box::new(move |_callback| {
            println!("Disconnected from Steam");

            is_running.store(false, Ordering::SeqCst);
        }));
    }

    {
        let steam_unified_messages = steam_unified_messages.clone();
        let steam_user = steam_user.clone();
        let is_running = is_running.clone();

        steam_client.subscribe::<LoggedOnCallback>(Box::new(move |callback| {
            if callback.result() != EResult::OK {
                if callback.result() == EResult::AccountLogonDenied {
                    // if we receive AccountLogonDenied or one of its flavors
                    // (AccountLogonDeniedNoMailSent, etc) then the account we're
                    // logging into is SteamGuard protected. See sample 5 for how
                    // SteamGuard can be handled
                    println!("Unable to logon to Steam: This account is SteamGuard protected.");

                    is_running.store(false, Ordering::SeqCst);
                    return;
                }

                println!(
                    "Unable to logon to Steam: {:?} / {:?}",
                    callback.result(),
                    callback.extended_result()
                );

                is_running.store(false, Ordering::SeqCst);
                return;
            }

            let steam_unified_messages = steam_unified_messages.clone();
            let steam_user = steam_user.clone();
            let is_running = is_running.clone();

            tokio::spawn(async move {
                on_logged_on(steam_unified_messages, steam_user, is_running).await;
            });
        }));
    }

    {
        steam_client.subscribe::<LoggedOffCallback>(Box::new(move |callback| {
            println!("Logged off of Steam: {:?}", callback.result());
        }));
    }

    println!("Connecting to Steam...");

    // initiate the connection
    steam_client.connect(None).await?;

    // create our callback handling loop
    while is_running.load(Ordering::SeqCst) {
        steam_client
            .run_wait_all_callbacks(Duration::from_secs(1))
            .await;
    }

    Ok(())
}
