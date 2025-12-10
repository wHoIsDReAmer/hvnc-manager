use serde::{Deserialize, Serialize};

use super::handshake::{Hello, HelloAck};
use super::input::InputEvent;
use super::liveness::KeepAlive;
use super::types::ErrorCode;
use super::video::FrameSegment;

/// High-level messages that travel over the relay tunnel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WireMessage {
    Hello(Hello),
    HelloAck(HelloAck),
    KeepAlive(KeepAlive),
    /// Manager → Client input.
    Input(InputEvent),
    /// Client → Manager framebuffer data (possibly deltas).
    Frame(FrameSegment),
    /// Client asks manager to request next frame (pull) or informs new frame ready (push).
    FrameReady {
        sequence: u64,
    },
    Error {
        code: ErrorCode,
        message: Option<String>,
    },
}
