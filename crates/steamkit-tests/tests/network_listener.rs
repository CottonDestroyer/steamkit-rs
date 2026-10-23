//! Debug network listener tests.
//!
//! `CMClient::send` notifies the debug listener *before* it fails with "not
//! connected", so the test ignores the send error and inspects the listener.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use steamkit::base::client_msg::ClientMsgProtobuf;
use steamkit::base::generated::steam_language::EMsg;
use steamkit::steam::steam_client::steam_client::SteamClient;
use steamkit::util::debug_network_listener::IDebugNetworkListener;
use steamkit_protos::steam::CMsgClientGamesPlayed;

/// Records the last outgoing message and whether the outgoing payload was
/// non-empty.
struct TestNetworkListener {
    last_outgoing: Mutex<Option<EMsg>>,
    outgoing_calls: AtomicUsize,
    saw_non_empty_outgoing: AtomicBool,
}

impl TestNetworkListener {
    fn new() -> Self {
        TestNetworkListener {
            last_outgoing: Mutex::new(None),
            outgoing_calls: AtomicUsize::new(0),
            saw_non_empty_outgoing: AtomicBool::new(false),
        }
    }
}

impl IDebugNetworkListener for TestNetworkListener {
    fn on_incoming_network_message(&self, _msg_type: EMsg, _data: &[u8]) {
        // Not needed here.
    }

    fn on_outgoing_network_message(&self, msg_type: EMsg, data: &[u8]) {
        *self
            .last_outgoing
            .lock()
            .expect("the listener lock must not be poisoned") = Some(msg_type);
        self.outgoing_calls.fetch_add(1, Ordering::SeqCst);

        if !data.is_empty() {
            self.saw_non_empty_outgoing.store(true, Ordering::SeqCst);
        }
    }
}

#[test]
fn network_listener_calls_on_outgoing_message() {
    let listener = Arc::new(TestNetworkListener::new());

    let client = SteamClient::new();
    client.set_debug_network_listener(Some(listener.clone() as Arc<dyn IDebugNetworkListener>));

    let client_msg = ClientMsgProtobuf::<CMsgClientGamesPlayed>::new(EMsg::ClientGamesPlayedNoDataBlob);

    // The client is not connected, so send() reports a connection error, but the
    // debug listener is notified before that; the error is intentionally ignored.
    let send_result = client.send(&client_msg);
    assert!(
        send_result.is_err(),
        "send without a connection must fail, but the listener is still notified"
    );

    assert_eq!(
        *listener
            .last_outgoing
            .lock()
            .expect("the listener lock must not be poisoned"),
        Some(EMsg::ClientGamesPlayedNoDataBlob)
    );
    assert!(
        listener.saw_non_empty_outgoing.load(Ordering::SeqCst),
        "the outgoing payload must be non-empty"
    );
}
