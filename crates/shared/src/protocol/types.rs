use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

/// Protocol version negotiated during handshake.
pub const PROTOCOL_VERSION: u16 = 1;

/// Session identifier allocated by the relay server.
pub type SessionId = u64;

/// Milliseconds since Unix epoch (UTC).
pub type TimestampMs = u64;

/// Actor role sent in the initial hello.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum Role {
    Manager = 1,
    Client = 2,
    Relay = 3,
}

/// Result codes used in error replies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize_repr, Deserialize_repr)]
#[repr(u16)]
pub enum ErrorCode {
    Unknown = 0,
    Unauthorized = 1,
    IncompatibleVersion = 2,
    Busy = 3,
    InvalidMessage = 4,
}

/// A rectangular region in screen coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Frame buffer pixel format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum FrameFormat {
    Rgba8888 = 1,
}
