use serde::{Deserialize, Serialize};

use super::handshake::{Hello, HelloAck};
use super::input::InputEvent;
use super::liveness::KeepAlive;
use super::session::{
    ClientList, ClientStatusChanged, ConnectRequest, DisconnectRequest, SessionEnded,
    SessionStarted,
};
use super::types::ErrorCode;
use super::video::FrameSegment;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WireMessage {
    Hello(Hello),
    HelloAck(HelloAck),
    KeepAlive(KeepAlive),

    ClientList(ClientList),
    ClientStatusChanged(ClientStatusChanged),
    Connect(ConnectRequest),
    SessionStarted(SessionStarted),
    Disconnect(DisconnectRequest),
    SessionEnded(SessionEnded),

    Input(InputEvent),
    Frame(FrameSegment),
    FrameReady {
        sequence: u64,
    },

    Error {
        code: ErrorCode,
        message: Option<String>,
    },
}
