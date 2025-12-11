use shared::protocol::ClientInfo;

pub enum NetworkEvent {
    Connected,
    Disconnected,
    ClientListUpdated(Vec<ClientInfo>),
    ClientStatusChanged {
        client_id: u64,
        online: bool,
        info: Option<ClientInfo>,
    },
    SessionStarted {
        client_id: u64,
        peer_name: String,
    },
    SessionEnded {
        reason: String,
    },
    FrameReceived {
        width: u32,
        height: u32,
        data: Vec<u8>,
    },
    Error(String),
}
