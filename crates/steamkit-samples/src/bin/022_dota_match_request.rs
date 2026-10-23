//! Requests the details of a Dota 2 match from the Dota game coordinator.

use anyhow::Result;
use steamkit_protos::dota2::CMsgDotaMatch;

mod dota_client {
    //! The client of this sample.

    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use steamkit::base::client_msg::ClientMsgProtobuf;
    use steamkit::base::generated::steam_language::{EMsg, EResult};
    use steamkit::base::gc::client_msg_gc::ClientGCMsgProtobuf;
    use steamkit::steam::handlers::steam_game_coordinator::callbacks::MessageCallback;
    use steamkit::steam::handlers::steam_game_coordinator::steam_game_coordinator::SteamGameCoordinator;
    use steamkit::steam::handlers::steam_user::callbacks::LoggedOnCallback;
    use steamkit::steam::handlers::steam_user::steam_user::{LogOnDetails, SteamUser};
    use steamkit::steam::steam_client::callbacks::ConnectedCallback;
    use steamkit::steam::steam_client::steam_client::SteamClient;
    use steamkit::types::game_id::GameID;
    use steamkit_protos::dota2::{
        CMsgClientHello, CMsgClientWelcome, CMsgDotaMatch, CMsgGcMatchDetailsRequest,
        CMsgGcMatchDetailsResponse, EgcBaseClientMsg, EdotagcMsg, ESourceEngine,
    };
    use steamkit_protos::steam::{c_msg_client_games_played, CMsgClientGamesPlayed};

    /// dota2's appid
    const APPID: u32 = 570;

    /// The game coordinator message that welcomes a client.
    const GC_CLIENT_WELCOME: u32 = EgcBaseClientMsg::KEMsgGcClientWelcome as u32;

    /// The game coordinator message that carries the details of a match.
    const GC_MATCH_DETAILS_RESPONSE: u32 = EdotagcMsg::KEMsgGcMatchDetailsResponse as u32;

    /// The client that logs on to Steam and asks the Dota game coordinator for
    /// the details of a match.
    pub struct DotaClient {
        client: Arc<SteamClient>,
        got_match: Arc<AtomicBool>,
        match_details: Arc<Mutex<Option<CMsgDotaMatch>>>,
    }

    impl DotaClient {
        /// Creates the client and registers the callbacks it is interested in.
        pub fn new(
            user_name: String,
            password: String,
            match_id: u64,
        ) -> Result<Self, steamkit::error::SteamError> {
            let client = SteamClient::new();

            // get our handlers
            let user = client.get_required_handler::<SteamUser>()?;
            let game_coordinator = client.get_required_handler::<SteamGameCoordinator>()?;

            let got_match = Arc::new(AtomicBool::new(false));
            let match_details = Arc::new(Mutex::new(None));

            // setup callbacks
            {
                let user = user.clone();
                let user_name = user_name.clone();
                let password = password.clone();

                client.subscribe::<ConnectedCallback>(Box::new(move |_callback| {
                    println!("Connected! Logging '{user_name}' into Steam...");

                    // we've successfully connected, so now attempt to logon
                    let details = LogOnDetails {
                        username: Some(user_name.clone()),
                        password: Some(password.clone()),
                        ..Default::default()
                    };

                    let _ = user.log_on(&details);
                }));
            }

            {
                let subscriber = client.clone();
                let client = client.clone();
                let game_coordinator = game_coordinator.clone();
                let got_match = got_match.clone();

                subscriber.subscribe::<LoggedOnCallback>(Box::new(move |callback| {
                    if callback.result() != EResult::OK {
                        // logon failed (password incorrect, steamguard enabled, etc)
                        // an EResult of AccountLogonDenied means the account has SteamGuard enabled and an email containing the auth code was sent
                        // in that case, you would get the auth code from the email and provide it in the LogOnDetails
                        println!("Unable to logon to Steam: {:?}", callback.result());

                        // we didn't actually get the match details, but we need to jump out of the callback loop
                        got_match.store(true, Ordering::SeqCst);
                        return;
                    }

                    println!("Logged in! Launching DOTA...");

                    // we've logged into the account. now we need to inform the
                    // steam server that we're playing dota (in order to receive
                    // GC messages)
                    //
                    // steamkit doesn't expose the "play game" message through
                    // any handler, so we'll just send the message manually
                    let mut play_game =
                        ClientMsgProtobuf::<CMsgClientGamesPlayed>::new(EMsg::ClientGamesPlayed);

                    play_game
                        .body
                        .games_played
                        .push(c_msg_client_games_played::GamePlayed {
                            game_id: Some(GameID::from_app_id(APPID as i32).to_u64()),
                            ..Default::default()
                        });

                    // send it off
                    // notice here we're sending this message directly using the SteamClient
                    let _ = client.send(&play_game);

                    let game_coordinator = game_coordinator.clone();

                    tokio::spawn(async move {
                        // delay a little to give steam some time to establish a GC connection to us
                        tokio::time::sleep(Duration::from_secs(5)).await;

                        // inform the dota GC that we want a session
                        let mut client_hello = ClientGCMsgProtobuf::<CMsgClientHello>::new(
                            EgcBaseClientMsg::KEMsgGcClientHello as u32,
                        );
                        client_hello.body.engine = Some(ESourceEngine::KEseSource2 as i32);

                        let _ = game_coordinator.send(&client_hello, APPID);
                    });
                }));
            }

            {
                let subscriber = client.clone();
                let client = client.clone();
                let game_coordinator = game_coordinator.clone();
                let got_match = got_match.clone();
                let match_details = match_details.clone();

                subscriber.subscribe::<MessageCallback>(Box::new(move |callback| {
                    // setup our dispatch table for messages. this makes the code
                    // cleaner and easier to maintain
                    match callback.e_msg() {
                        GC_CLIENT_WELCOME => {
                            on_client_welcome(&game_coordinator, match_id, callback);
                        }
                        GC_MATCH_DETAILS_RESPONSE => {
                            on_match_details(&client, &got_match, &match_details, callback);
                        }
                        // this will happen when we receive some GC messages that
                        // we're not handling. this is okay because we're handling
                        // every essential message, and the rest can be ignored
                        _ => {}
                    }
                }));
            }

            Ok(DotaClient {
                client,
                got_match,
                match_details,
            })
        }

        /// The details of the match, if they have been received.
        pub fn match_details(&self) -> Option<CMsgDotaMatch> {
            match self.match_details.lock() {
                Ok(guard) => guard.clone(),
                Err(poisoned) => poisoned.into_inner().clone(),
            }
        }

        /// Begins the connection to Steam.
        pub async fn connect(&self) {
            println!("Connecting to Steam...");

            // begin the connection to steam
            let _ = self.client.connect(None).await;
        }

        /// Waits for the results of the match request.
        pub async fn wait(&self) {
            while !self.got_match.load(Ordering::SeqCst) {
                // continue running callbacks until we get match details
                self.client
                    .run_wait_all_callbacks(Duration::from_secs(1))
                    .await;
            }
        }
    }

    /// This message arrives when the GC welcomes a client.
    ///
    /// This happens after telling steam that we launched dota (with the
    /// ClientGamesPlayed message). This can also happen after the GC has
    /// restarted (due to a crash or new version).
    fn on_client_welcome(
        game_coordinator: &SteamGameCoordinator,
        match_id: u64,
        callback: &MessageCallback,
    ) {
        // in order to get at the contents of the message, we need to create a ClientGCMsgProtobuf from the packet message we receive
        // note here the difference between ClientGCMsgProtobuf and the ClientMsgProtobuf used when sending ClientGamesPlayed
        // this message is used for the GC, while the other is used for general steam messages
        let Ok(msg) = ClientGCMsgProtobuf::<CMsgClientWelcome>::from_packet(callback.message()) else {
            return;
        };

        println!(
            "GC is welcoming us. Version: {}",
            msg.body().version.unwrap_or_default()
        );

        println!("Requesting details of match {match_id}");

        // at this point, the GC is now ready to accept messages from us. so now
        // we'll request the details of the match we're looking for
        let mut request_match = ClientGCMsgProtobuf::<CMsgGcMatchDetailsRequest>::new(
            EdotagcMsg::KEMsgGcMatchDetailsRequest as u32,
        );
        request_match.body.match_id = Some(match_id);

        let _ = game_coordinator.send(&request_match, APPID);
    }

    /// This message arrives after we've requested the details for a match.
    fn on_match_details(
        client: &Arc<SteamClient>,
        got_match: &AtomicBool,
        match_details: &Mutex<Option<CMsgDotaMatch>>,
        callback: &MessageCallback,
    ) {
        let Ok(msg) =
            ClientGCMsgProtobuf::<CMsgGcMatchDetailsResponse>::from_packet(callback.message())
        else {
            return;
        };

        let result = EResult::from_value(msg.body().result.unwrap_or_default() as i32)
            .unwrap_or(EResult::Invalid);

        if result != EResult::OK {
            println!("Unable to request match details: {result:?}");
        }

        got_match.store(true, Ordering::SeqCst);

        match match_details.lock() {
            Ok(mut guard) => *guard = msg.body().r#match.clone(),
            Err(poisoned) => *poisoned.into_inner() = msg.body().r#match.clone(),
        }

        // we've got everything we need, we can disconnect from steam now
        let client = client.clone();

        tokio::spawn(async move {
            client.disconnect().await;
        });
    }
}

/// Prints off what steam gave us.
fn print_match_details(match_details: Option<&CMsgDotaMatch>) {
    let Some(match_details) = match_details else {
        println!("No match details to display");
        return;
    };

    // print the interesting fields of the match
    println!(
        "average_skill: {}",
        match_details.average_skill.unwrap_or_default()
    );
    println!("barracks_status: {:?}", match_details.barracks_status);
    println!("cluster: {}", match_details.cluster.unwrap_or_default());
    println!("duration: {}", match_details.duration.unwrap_or_default());
    println!("engine: {}", match_details.engine.unwrap_or_default());
    println!(
        "first_blood_time: {}",
        match_details.first_blood_time.unwrap_or_default()
    );
    println!(
        "game_balance: {}",
        match_details.game_balance.unwrap_or_default()
    );
    println!("game_mode: {}", match_details.game_mode.unwrap_or_default());
    println!(
        "human_players: {}",
        match_details.human_players.unwrap_or_default()
    );
    println!("leagueid: {}", match_details.leagueid.unwrap_or_default());
    println!(
        "match_flags: {}",
        match_details.match_flags.unwrap_or_default()
    );
    println!("match_id: {}", match_details.match_id.unwrap_or_default());
    println!(
        "match_outcome: {}",
        match_details.match_outcome.unwrap_or_default()
    );
    println!(
        "match_seq_num: {}",
        match_details.match_seq_num.unwrap_or_default()
    );
    println!("players: {}", match_details.players.len());
    println!(
        "pre_game_duration: {}",
        match_details.pre_game_duration.unwrap_or_default()
    );
    println!(
        "radiant_guild_id: {}",
        match_details.radiant_guild_id.unwrap_or_default()
    );
    println!(
        "radiant_team_complete: {}",
        match_details.radiant_team_complete.unwrap_or_default()
    );
    println!(
        "radiant_team_id: {}",
        match_details.radiant_team_id.unwrap_or_default()
    );
    println!(
        "radiant_team_logo: {}",
        match_details.radiant_team_logo.unwrap_or_default()
    );
    println!(
        "radiant_team_logo_url: {}",
        match_details.radiant_team_logo_url.as_deref().unwrap_or_default()
    );
    println!(
        "radiant_team_name: {}",
        match_details.radiant_team_name.as_deref().unwrap_or_default()
    );
    println!(
        "radiant_team_score: {}",
        match_details.radiant_team_score.unwrap_or_default()
    );
    println!(
        "radiant_team_tag: {}",
        match_details.radiant_team_tag.as_deref().unwrap_or_default()
    );
    println!(
        "replay_salt: {}",
        match_details.replay_salt.unwrap_or_default()
    );
    println!(
        "replay_state: {}",
        match_details.replay_state.unwrap_or_default()
    );
    println!("server_ip: {}", match_details.server_ip.unwrap_or_default());
    println!("server_port: {}", match_details.server_port.unwrap_or_default());
    println!("series_id: {}", match_details.series_id.unwrap_or_default());
    println!("series_type: {}", match_details.series_type.unwrap_or_default());
    println!("starttime: {}", match_details.starttime.unwrap_or_default());
    println!("tower_status: {:?}", match_details.tower_status);
    println!(
        "tournament_id: {}",
        match_details.tournament_id.unwrap_or_default()
    );
    println!(
        "tournament_round: {}",
        match_details.tournament_round.unwrap_or_default()
    );
}

/// Prints the usage of this sample.
fn print_usage() {
    println!("Usage:");
    println!("DotaMatchRequest <username> <password> <matchid>");
}

/// Requests the details of a Dota 2 match.
#[tokio::main]
async fn main() -> Result<()> {
    let mut arguments = std::env::args().skip(1);

    let (Some(username), Some(password), Some(match_id)) = (
        arguments.next(),
        arguments.next(),
        arguments.next(),
    ) else {
        print_usage();
        return Ok(());
    };

    let Ok(match_id) = match_id.parse::<u64>() else {
        println!("Invalid Match ID!");
        return Ok(());
    };

    let client = dota_client::DotaClient::new(username, password, match_id)?;

    // connect
    client.connect().await;

    // wait for results of the match request
    client.wait().await;

    // print off what steam gave us
    print_match_details(client.match_details().as_ref());

    Ok(())
}
