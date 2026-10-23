//! Builds on the login sample and adds basic interaction with the client's
//! persona state and friends list.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use steamkit::base::generated::steam_language::{EFriendRelationship, EPersonaState, EResult};
use steamkit::networking::steam3::protocol_type::ProtocolTypes;
use steamkit::steam::handlers::steam_friends::callbacks::{
    FriendAddedCallback, FriendsListCallback, PersonaStateCallback,
};
use steamkit::steam::handlers::steam_friends::steam_friends::SteamFriends;
use steamkit::steam::handlers::steam_user::callbacks::{
    AccountInfoCallback, LoggedOffCallback, LoggedOnCallback,
};
use steamkit::steam::handlers::steam_user::steam_user::{LogOnDetails, SteamUser};
use steamkit::steam::steam_client::callbacks::{ConnectedCallback, DisconnectedCallback};
use steamkit::steam::steam_client::configuration::i_steam_configuration_builder::ISteamConfigurationBuilder;
use steamkit::steam::steam_client::configuration::steam_configuration::SteamConfiguration;
use steamkit::steam::steam_client::steam_client::SteamClient;

/// Logs on to Steam and shows the friends list.
#[tokio::main]
async fn main() -> Result<()> {
    let mut arguments = std::env::args().skip(1);

    let (Some(user), Some(pass)) = (arguments.next(), arguments.next()) else {
        println!("Sample4: No username and password specified!");
        return Ok(());
    };

    // create our steamclient instance
    let configuration = SteamConfiguration::create(|builder| {
        builder.with_protocol_types(ProtocolTypes::TCP);
    });
    let steam_client = SteamClient::with_config(configuration);
    // get the steamuser handler, which is used for logging on after successfully connecting
    let steam_user = steam_client.get_required_handler::<SteamUser>()?;
    // get the steam friends handler, which is used for interacting with friends
    // on the network after logging on
    let steam_friends = steam_client.get_required_handler::<SteamFriends>()?;

    let is_running = Arc::new(AtomicBool::new(true));

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

            println!("Successfully logged on!");

            // at this point, we'd be able to perform actions on Steam

            // for this sample we wait for other callbacks to perform logic
            let _ = &steam_user;
        }));
    }

    {
        steam_client.subscribe::<LoggedOffCallback>(Box::new(move |callback| {
            println!("Logged off of Steam: {:?}", callback.result());
        }));
    }

    // we use the following callbacks for friends related activities
    {
        let steam_friends = steam_friends.clone();

        steam_client.subscribe::<AccountInfoCallback>(Box::new(move |_callback| {
            // before being able to interact with friends, you must wait for the
            // account info callback. this callback is posted shortly after a
            // successful logon
            //
            // at this point, we can go online on friends, so lets do that
            let _ = steam_friends.set_persona_state(EPersonaState::Online);
        }));
    }

    {
        let steam_friends = steam_friends.clone();

        steam_client.subscribe::<FriendsListCallback>(Box::new(move |callback| {
            // at this point, the client has received its friends list
            let friend_count = steam_friends.get_friend_count();

            println!("We have {friend_count} friends");

            for index in 0..friend_count {
                // steamids identify objects that exist on the steam network, such
                // as friends, as an example
                let steam_id_friend = steam_friends.get_friend_by_index(index);

                // we'll just display the STEAM_ rendered version
                println!("Friend: {}", steam_id_friend.render(false));
            }

            // we can also iterate over our friendslist to accept or decline any
            // pending invites
            for friend in callback.friend_list() {
                if friend.relationship() == EFriendRelationship::RequestRecipient {
                    // this user has added us, let's add them back
                    let _ = steam_friends.add_friend_by_steam_id(friend.steam_id());
                }
            }
        }));
    }

    {
        steam_client.subscribe::<FriendAddedCallback>(Box::new(move |callback| {
            // someone accepted our friend request, or we accepted one
            println!("{} is now a friend", callback.persona_name());
        }));
    }

    {
        steam_client.subscribe::<PersonaStateCallback>(Box::new(move |callback| {
            // this callback is received when the persona state (friend
            // information) of a friend changes
            //
            // for this sample we'll simply display the names of the friends
            println!("State change: {}", callback.name());
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
