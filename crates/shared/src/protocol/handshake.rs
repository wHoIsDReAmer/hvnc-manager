use serde::{Deserialize, Serialize};

use super::types::{ClientId, Role};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub version: u16,
    pub role: Role,
    pub auth_token: String,
    pub node_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloAck {
    pub accepted: bool,
    pub client_id: Option<ClientId>,
    pub reason: Option<String>,
    pub negotiated_version: u16,
}
