pub enum NetworkCommand {
    Connect { addr: String, token: String },
    Disconnect,
    ConnectToClient { client_id: u64 },
    DisconnectFromClient,
    MouseMove { x: i32, y: i32 },
    MouseClick { button: u8, down: bool },
    KeyEvent { key: String, down: bool },
}
