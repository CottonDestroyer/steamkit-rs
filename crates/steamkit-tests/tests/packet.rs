//! Feeds every inbound packet capture in `tests/data` (`..._in_...` file
//! names) to a `SteamClient` and checks that the handlers post exactly the
//! callback expected for that message type.

mod common;

use std::path::PathBuf;

use steamkit::base::generated::steam_language::EMsg;
use steamkit::base::packet_base::{IPacketMsg, PacketClientMsgProtobuf};
use steamkit::steam::handlers::steam_apps::callbacks::PICSProductInfoCallback;
use steamkit::steam::steam_client::callback_mgr::callback_msg::CallbackMsg;
use steamkit::steam::steam_client::steam_client::SteamClient;

/// A captured packet and its message type.
struct TestPacket {
    e_msg: EMsg,
    data: Vec<u8>,
}

/// The callback expected for a message type.
enum ExpectedCallback {
    PicsProductInfo,
}

impl ExpectedCallback {
    /// Whether `callback` is of the expected concrete type.
    fn matches(&self, callback: &dyn CallbackMsg) -> bool {
        match self {
            ExpectedCallback::PicsProductInfo => callback
                .as_any()
                .downcast_ref::<PICSProductInfoCallback>()
                .is_some(),
        }
    }
}

/// The callback expected for a message type, if any.
fn get_callback(msg: EMsg) -> Option<ExpectedCallback> {
    match msg {
        EMsg::ClientPICSProductInfoResponse => Some(ExpectedCallback::PicsProductInfo),
        _ => None,
    }
}

/// Loads a capture file if it belongs to `direction`.
fn get_packet(filename: &str, direction: &str) -> Option<TestPacket> {
    let path = PathBuf::from(filename);
    let stem = path.file_stem()?.to_str()?;
    let parts: Vec<&str> = stem.split('_').collect();

    assert!(parts.len() > 3, "'{stem}' must have more than three parts");

    if parts[1] != direction {
        return None;
    }

    let emsg = EMsg::from_value(parts[2].parse::<u32>().ok()? as i32)?;

    let data = std::fs::read(&path)
        .unwrap_or_else(|error| panic!("failed to read test data file {filename}: {error}"));

    Some(TestPacket { e_msg: emsg, data })
}

/// Loads every capture file of a direction.
fn get_packets(direction: &str) -> Vec<TestPacket> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(common::data_dir())
        .expect("the tests/data directory must be readable")
        .map(|entry| entry.expect("a directory entry must be readable").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "bin"))
        .collect();

    // A deterministic order keeps the failures reproducible.
    files.sort();

    files
        .iter()
        .filter_map(|path| get_packet(path.to_str()?, direction))
        .collect()
}

#[tokio::test]
async fn posts_expected_callbacks() {
    let steam_client = SteamClient::new();

    for TestPacket { e_msg, data } in get_packets("in") {
        let expected_callback =
            get_callback(e_msg).expect("every inbound fixture must have a known callback");

        let packet_msg = steam_client
            .cm_client()
            .get_packet_msg(data.clone())
            .expect("every inbound fixture must parse as a packet message");

        assert!(
            packet_msg.is_proto(),
            "every inbound fixture must be a PacketClientMsgProtobuf"
        );
        assert_eq!(e_msg, packet_msg.msg_type());

        // The packet must parse as a protobuf client message.
        let as_protobuf = PacketClientMsgProtobuf::new(e_msg, data)
            .expect("the fixture must parse as a PacketClientMsgProtobuf");
        assert!(as_protobuf.is_proto());
        assert_eq!(packet_msg.get_data(), as_protobuf.get_data());

        assert!(
            steam_client.get_callback().is_none(),
            "there must be no callbacks queued"
        );

        steam_client
            .receive_test_packet_msg(packet_msg.as_ref())
            .await;

        let callback = steam_client
            .get_callback()
            .expect("the handler pipeline must post a callback");

        assert!(
            expected_callback.matches(callback.as_ref()),
            "the posted callback must be of the expected type"
        );
    }
}
