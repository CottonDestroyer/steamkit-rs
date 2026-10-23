//! Arguments of the `NetMsgReceived` event.

use std::net::SocketAddr;

/// Data received from the network.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetMsgEventArgs {
    /// The raw bytes that were received.
    pub data: Vec<u8>,
    /// The endpoint the data was received from.
    pub end_point: SocketAddr,
}

impl NetMsgEventArgs {
    /// Creates a new instance.
    pub fn new(data: Vec<u8>, end_point: SocketAddr) -> Self {
        Self { data, end_point }
    }

    /// A copy with different data and the same endpoint.
    pub fn with_data(&self, data: Vec<u8>) -> NetMsgEventArgs {
        NetMsgEventArgs {
            data,
            end_point: self.end_point,
        }
    }
}
