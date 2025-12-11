use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

pub const PROTOCOL_VERSION: u16 = 1;

pub type SessionId = u64;
pub type ClientId = u64;
pub type TimestampMs = u64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientInfo {
    pub client_id: ClientId,
    pub node_name: String,
    pub connected_at: TimestampMs,
    pub is_busy: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerInfo {
    pub node_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum Role {
    Manager = 1,
    Client = 2,
    Relay = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize_repr, Deserialize_repr)]
#[repr(u16)]
pub enum ErrorCode {
    Unknown = 0,
    Unauthorized = 1,
    IncompatibleVersion = 2,
    Busy = 3,
    InvalidMessage = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum FrameFormat {
    Rgba8888 = 1,
}
