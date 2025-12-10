use serde::{Deserialize, Serialize};

use super::types::{Role, SessionId};

/// Initial handshake from any peer to the relay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub version: u16,
    pub role: Role,
    /// Optional bearer token or shared secret; empty = unauthenticated.
    pub auth_token: String,
    /// Human-friendly name for logs/observability.
    pub node_name: String,
}

/// Reply to `Hello`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloAck {
    pub accepted: bool,
    pub session_id: Option<SessionId>,
    pub reason: Option<String>,
    pub negotiated_version: u16,
}
