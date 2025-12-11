use std::sync::Arc;

use anyhow::Result;
use bytes::BytesMut;
use quinn::{Connection, Endpoint, SendStream};
use shared::protocol::{
    ClientId, ClientList, ConnectRequest, DisconnectRequest, Hello, HelloAck, InputEvent,
    KeyAction, KeyboardEvent, MouseAction, MouseButton, MouseEvent, PROTOCOL_VERSION, Role,
    WireMessage,
};
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, mpsc};
use tracing::{info, warn};

use super::command::NetworkCommand;
use super::endpoint::create_client_endpoint;
use super::event::NetworkEvent;
use super::recv::{read_message_with_buf, receive_loop_with_buf};

pub struct NetworkManager {
    cmd_rx: mpsc::Receiver<NetworkCommand>,
    event_tx: mpsc::Sender<NetworkEvent>,
    connection: Option<Connection>,
    control_tx: Option<Arc<Mutex<SendStream>>>,
    endpoint: Option<Endpoint>,
}

impl NetworkManager {
    pub fn new(
        cmd_rx: mpsc::Receiver<NetworkCommand>,
        event_tx: mpsc::Sender<NetworkEvent>,
    ) -> Self {
        Self {
            cmd_rx,
            event_tx,
            connection: None,
            control_tx: None,
            endpoint: None,
        }
    }

    pub async fn run(&mut self) {
        while let Some(cmd) = self.cmd_rx.recv().await {
            if let Err(e) = self.handle_command(cmd).await {
                let _ = self.event_tx.send(NetworkEvent::Error(e.to_string())).await;
            }
        }
    }

    async fn handle_command(&mut self, cmd: NetworkCommand) -> Result<()> {
        match cmd {
            NetworkCommand::Connect { addr, token } => {
                self.connect(&addr, &token).await?;
            }
            NetworkCommand::Disconnect => {
                self.disconnect().await;
            }
            NetworkCommand::ConnectToClient { client_id } => {
                self.connect_to_client(client_id).await?;
            }
            NetworkCommand::DisconnectFromClient => {
                self.disconnect_from_client().await?;
            }
            NetworkCommand::MouseMove { x, y } => {
                self.send_mouse_move(x, y).await?;
            }
            NetworkCommand::MouseClick { button, down } => {
                self.send_mouse_click(button, down).await?;
            }
            NetworkCommand::KeyEvent { key, down } => {
                self.send_key_event(&key, down).await?;
            }
        }
        Ok(())
    }

    async fn connect(&mut self, addr: &str, token: &str) -> Result<()> {
        info!("Connecting to relay at {}", addr);
        let endpoint = create_client_endpoint()?;
        let connection = endpoint.connect(addr.parse()?, "localhost")?.await?;
        info!("QUIC connection established");

        let (send, mut recv) = connection.open_bi().await?;
        info!("Bi-stream opened");

        let hello = WireMessage::Hello(Hello {
            version: PROTOCOL_VERSION,
            role: Role::Manager,
            auth_token: token.to_string(),
            node_name: "hvnc-manager".to_string(),
        });
        let bytes = shared::encode_to_vec(&hello)?;

        let control_tx = Arc::new(Mutex::new(send));
        self.control_tx = Some(Arc::clone(&control_tx));
        self.endpoint = Some(endpoint);
        self.connection = Some(connection.clone());
        {
            let mut guard = control_tx.lock().await;
            guard.write_all(&bytes).await?;
            guard.flush().await?;
        }
        info!("Hello sent");

        let mut buf = BytesMut::with_capacity(8192);
        let ack = read_message_with_buf(&mut recv, &mut buf).await?;
        info!("HelloAck received");
        if let WireMessage::HelloAck(HelloAck {
            accepted, reason, ..
        }) = ack
            && !accepted
        {
            return Err(anyhow::anyhow!("Connection rejected: {:?}", reason));
        }

        let _ = self.event_tx.send(NetworkEvent::Connected).await;

        // Read client list (may already be in buf from previous read)
        let msg = read_message_with_buf(&mut recv, &mut buf).await?;
        info!(
            "Second message received: {:?}",
            std::mem::discriminant(&msg)
        );
        if let WireMessage::ClientList(ClientList { clients }) = msg {
            info!("ClientList received with {} clients", clients.len());
            let _ = self
                .event_tx
                .send(NetworkEvent::ClientListUpdated(clients))
                .await;
        }

        // Spawn datagram receiver for frames
        let event_tx_dg = self.event_tx.clone();
        let conn_for_dg = connection.clone();
        tokio::spawn(async move {
            while let Ok(data) = conn_for_dg.read_datagram().await {
                if let Ok(msg) = shared::decode_datagram(&data)
                    && let WireMessage::Frame(frame) = msg
                {
                    let _ = event_tx_dg
                        .send(NetworkEvent::FrameReceived {
                            width: frame.region.width,
                            height: frame.region.height,
                            data: frame.data.into_vec(),
                        })
                        .await;
                }
            }
        });

        // Spawn message receiver with remaining buffer
        let event_tx = self.event_tx.clone();
        tokio::spawn(async move {
            info!("Receive loop started");
            if let Err(e) = receive_loop_with_buf(recv, event_tx, buf).await {
                warn!("Receive loop ended: {}", e);
            }
        });

        info!("Connect completed successfully");
        Ok(())
    }

    async fn disconnect(&mut self) {
        if let Some(conn) = self.connection.take() {
            conn.close(0u32.into(), b"disconnect");
        }
        self.control_tx = None;
        self.endpoint = None;
        let _ = self.event_tx.send(NetworkEvent::Disconnected).await;
    }

    async fn send_control(&self, msg: &WireMessage) -> Result<()> {
        let tx = self
            .control_tx
            .as_ref()
            .ok_or(anyhow::anyhow!("Not connected"))?;
        let bytes = shared::encode_to_vec(msg)?;
        let mut guard = tx.lock().await;
        guard.write_all(&bytes).await?;
        guard.flush().await?;
        Ok(())
    }

    async fn connect_to_client(&mut self, client_id: ClientId) -> Result<()> {
        let msg = WireMessage::Connect(ConnectRequest {
            target_client_id: client_id,
        });
        self.send_control(&msg).await
    }

    async fn disconnect_from_client(&mut self) -> Result<()> {
        let msg = WireMessage::Disconnect(DisconnectRequest { reason: None });
        self.send_control(&msg).await
    }

    async fn send_mouse_move(&mut self, dx: i32, dy: i32) -> Result<()> {
        self.send_input(InputEvent::Mouse(MouseEvent::Move { dx, dy }))
            .await
    }

    async fn send_mouse_click(&mut self, button: u8, down: bool) -> Result<()> {
        let btn = match button {
            1 => MouseButton::Left,
            2 => MouseButton::Right,
            _ => MouseButton::Middle,
        };
        let action = if down {
            MouseAction::Down
        } else {
            MouseAction::Up
        };
        self.send_input(InputEvent::Mouse(MouseEvent::Button {
            button: btn,
            action,
        }))
        .await
    }

    async fn send_key_event(&mut self, _key: &str, down: bool) -> Result<()> {
        let action = if down { KeyAction::Down } else { KeyAction::Up };
        self.send_input(InputEvent::Keyboard(KeyboardEvent {
            scancode: 0,
            action,
        }))
        .await
    }

    async fn send_input(&mut self, input: InputEvent) -> Result<()> {
        let conn = self
            .connection
            .as_ref()
            .ok_or(anyhow::anyhow!("Not connected"))?;
        let msg = WireMessage::Input(input);
        let bytes = shared::encode_datagram(&msg)?;
        conn.send_datagram(bytes.into())?;
        Ok(())
    }
}
