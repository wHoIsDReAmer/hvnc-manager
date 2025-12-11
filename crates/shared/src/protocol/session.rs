use serde::{Deserialize, Serialize};

use super::types::{ClientId, ClientInfo, PeerInfo, SessionId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectRequest {
    pub target_client_id: ClientId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionStarted {
    pub session_id: SessionId,
    pub peer: PeerInfo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisconnectRequest {
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionEnded {
    pub session_id: SessionId,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientList {
    pub clients: Vec<ClientInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientStatusChanged {
    pub client_id: ClientId,
    pub online: bool,
    pub info: Option<ClientInfo>,
}
