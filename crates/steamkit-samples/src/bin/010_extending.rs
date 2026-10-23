//! Shows how to extend the client with a custom message handler and a custom
//! callback.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use steamkit::base::generated::steam_language::EResult;
use steamkit::steam::handlers::steam_user::callbacks::{LoggedOffCallback, LoggedOnCallback};
use steamkit::steam::handlers::steam_user::steam_user::{LogOnDetails, SteamUser};
use steamkit::steam::steam_client::callbacks::{ConnectedCallback, DisconnectedCallback};
use steamkit::steam::steam_client::steam_client::SteamClient;

mod my_handler {
    //! The custom handler and callback of this sample.

    use std::any::Any;
    use std::sync::{Arc, Weak};

    use steamkit::base::client_msg::ClientMsgProtobuf;
    use steamkit::base::generated::steam_language::{EMsg, EResult};
    use steamkit::base::packet_base::IPacketMsg;
    use steamkit::error::SteamResult;
    use steamkit::steam::handlers::client_msg_handler::ClientMsgHandler;
    use steamkit::steam::steam_client::callback_mgr::callback_msg::{CallbackMsg, CallbackMsgBase};
    use steamkit::steam::steam_client::steam_client::SteamClient;
    use steamkit::types::job_id::JobID;
    use steamkit_protos::steam::{CMsgClientLogOff, CMsgClientLogonResponse};

    /// The boxed future that the `ClientMsgHandler` trait expects.
    ///
    /// `ClientMsgHandler` is declared with `#[async_trait]`; this crate doesn't
    /// depend on `async-trait`, so the desugared signature is written out.
    type HandlerFuture<'a> =
        std::pin::Pin<Box<dyn std::future::Future<Output = SteamResult<()>> + Send + 'a>>;

    /// The custom callback of this sample.
    ///
    /// It passes the result of the logon back to the user of the handler.
    #[derive(Clone, Copy, Debug)]
    pub struct MyCallback {
        base: CallbackMsgBase,
        result: EResult,
    }

    impl MyCallback {
        /// Creates a callback that carries the result of the logon.
        pub fn new(result: EResult) -> Self {
            Self {
                base: CallbackMsgBase::new(),
                result,
            }
        }

        /// The result of the logon.
        pub fn result(&self) -> EResult {
            self.result
        }
    }

    impl CallbackMsg for MyCallback {
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

    /// A handler that reacts to the logon response of the Steam network.
    pub struct MyHandler {
        client: Weak<SteamClient>,
    }

    impl MyHandler {
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
                .ok_or_else(|| steamkit::error::SteamError::Connection("SteamClient has been dropped".to_string()))
        }

        /// Handlers can also define functions which can send data to the steam servers.
        #[allow(dead_code)]
        pub fn log_off(&self, _user: &str, _pass: &str) -> SteamResult<()> {
            let log_off_message = ClientMsgProtobuf::<CMsgClientLogOff>::new(EMsg::ClientLogOff);

            self.client()?.send(&log_off_message)
        }

        /// Some other useful function.
        pub fn do_something(&self) -> SteamResult<()> {
            self.client()?
                .log_debug("MyHandler", "MyHandler.DoSomething was called");
            Ok(())
        }

        /// This function is called when a message arrives from the Steam network.
        ///
        /// The client passes every incoming message to each registered
        /// handler.
        fn handle_logon_response(
            &self,
            client: &SteamClient,
            packet_msg: &dyn IPacketMsg,
        ) -> SteamResult<()> {
            // in order to get at the message contents, we need to wrap the packet
            // message in an object that gives us access to the message body
            let logon_response = ClientMsgProtobuf::<CMsgClientLogonResponse>::from_packet(packet_msg)?;

            // the raw body of the message often doesn't make use of useful types,
            // so we need to cast them to types that are prettier for the user to
            // handle
            let result = EResult::from_value(logon_response.body.eresult.unwrap_or_default())
                .unwrap_or(EResult::Invalid);

            // our handler will simply display a message in the console, and then
            // post our custom callback with the result of logon
            println!("HandleLogonResponse: {result:?}");

            // post the callback to be consumed by user code
            client.post_callback(Box::new(MyCallback::new(result)));

            Ok(())
        }
    }

    impl ClientMsgHandler for MyHandler {
        fn handle_msg<'life0, 'life1, 'life2, 'async_trait>(
            &'life0 self,
            client: &'life1 SteamClient,
            packet_msg: &'life2 dyn IPacketMsg,
        ) -> HandlerFuture<'async_trait>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            'life2: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move {
                // the MsgType exposes the EMsg (type) of the message
                match packet_msg.msg_type() {
                    // we want to custom handle this message, for the sake of an example
                    EMsg::ClientLogOnResponse => self.handle_logon_response(client, packet_msg)?,
                    _ => {}
                }

                Ok(())
            })
        }
    }
}

use my_handler::{MyCallback, MyHandler};

/// Logs on to Steam, shows the custom callback and logs off again.
#[tokio::main]
async fn main() -> Result<()> {
    let mut arguments = std::env::args().skip(1);

    let (Some(user), Some(pass)) = (arguments.next(), arguments.next()) else {
        println!("Sample2: No username and password specified!");
        return Ok(());
    };

    // create our steamclient instance
    let steam_client = SteamClient::new();

    // add our custom handler to our steamclient
    steam_client.add_typed_handler(MyHandler::new(&steam_client))?;

    // get the steamuser handler, which is used for logging on after successfully connecting
    let steam_user = steam_client.get_required_handler::<SteamUser>()?;
    // now get an instance of our custom handler
    let my_handler = steam_client.get_handler::<MyHandler>();

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

            // for this sample we'll just log off
            let _ = steam_user.log_off();
        }));
    }

    {
        steam_client.subscribe::<LoggedOffCallback>(Box::new(move |callback| {
            println!("Logged off of Steam: {:?}", callback.result());
        }));
    }

    // handle our own custom callback
    {
        steam_client.subscribe::<MyCallback>(Box::new(move |callback| {
            // this will be called when our custom callback gets posted
            println!("OnMyCallback: {:?}", callback.result());
        }));
    }

    if let Some(my_handler) = my_handler {
        my_handler.do_something()?;
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
