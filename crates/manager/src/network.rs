use anyhow::Result;
use bytes::BytesMut;
use quinn::{Connection, Endpoint};
use shared::protocol::{
    ClientId, ClientInfo, ClientList, ClientStatusChanged, ConnectRequest, DisconnectRequest,
    Hello, HelloAck, InputEvent, KeyAction, KeyboardEvent, MouseAction, MouseButton, MouseEvent,
    PROTOCOL_VERSION, Role, SessionEnded, SessionStarted, WireMessage,
};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, warn};

pub enum NetworkCommand {
    Connect { addr: String, token: String },
    Disconnect,
    ConnectToClient { client_id: u64 },
    DisconnectFromClient,
    MouseMove { x: i32, y: i32 },
    MouseClick { button: u8, down: bool },
    KeyEvent { key: String, down: bool },
}

pub enum NetworkEvent {
    Connected,
    Disconnected,
    ClientListUpdated(Vec<ClientInfo>),
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

pub struct NetworkManager {
    cmd_rx: mpsc::Receiver<NetworkCommand>,
    event_tx: mpsc::Sender<NetworkEvent>,
    connection: Option<Connection>,
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
        let endpoint = create_client_endpoint()?;
        let connection = endpoint.connect(addr.parse()?, "localhost")?.await?;

        let (mut send, mut recv) = connection.open_bi().await?;

        let hello = WireMessage::Hello(Hello {
            version: PROTOCOL_VERSION,
            role: Role::Manager,
            auth_token: token.to_string(),
            node_name: "hvnc-manager".to_string(),
        });
        let bytes = shared::encode_to_vec(&hello)?;
        send.write_all(&bytes).await?;

        let ack = read_message(&mut recv).await?;
        if let WireMessage::HelloAck(HelloAck {
            accepted, reason, ..
        }) = ack
            && !accepted
        {
            return Err(anyhow::anyhow!("Connection rejected: {:?}", reason));
        }

        self.endpoint = Some(endpoint);
        self.connection = Some(connection.clone());
        let _ = self.event_tx.send(NetworkEvent::Connected).await;

        // Read client list
        let msg = read_message(&mut recv).await?;
        if let WireMessage::ClientList(ClientList { clients }) = msg {
            let _ = self
                .event_tx
                .send(NetworkEvent::ClientListUpdated(clients))
                .await;
        }

        // Spawn message receiver
        let event_tx = self.event_tx.clone();
        tokio::spawn(async move {
            if let Err(e) = receive_loop(recv, event_tx).await {
                warn!("Receive loop ended: {}", e);
            }
        });

        Ok(())
    }

    async fn disconnect(&mut self) {
        if let Some(conn) = self.connection.take() {
            conn.close(0u32.into(), b"disconnect");
        }
        self.endpoint = None;
        let _ = self.event_tx.send(NetworkEvent::Disconnected).await;
    }

    async fn connect_to_client(&mut self, client_id: ClientId) -> Result<()> {
        let conn = self
            .connection
            .as_ref()
            .ok_or(anyhow::anyhow!("Not connected"))?;
        let (mut send, _) = conn.open_bi().await?;

        let msg = WireMessage::Connect(ConnectRequest {
            target_client_id: client_id,
        });
        let bytes = shared::encode_to_vec(&msg)?;
        send.write_all(&bytes).await?;

        Ok(())
    }

    async fn disconnect_from_client(&mut self) -> Result<()> {
        let conn = self
            .connection
            .as_ref()
            .ok_or(anyhow::anyhow!("Not connected"))?;
        let (mut send, _) = conn.open_bi().await?;

        let msg = WireMessage::Disconnect(DisconnectRequest { reason: None });
        let bytes = shared::encode_to_vec(&msg)?;
        send.write_all(&bytes).await?;

        Ok(())
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
        // TODO: Convert key string to scancode
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

async fn receive_loop(
    mut recv: quinn::RecvStream,
    event_tx: mpsc::Sender<NetworkEvent>,
) -> Result<()> {
    let mut buf = BytesMut::with_capacity(64 * 1024);

    loop {
        match recv.read_chunk(8192, true).await? {
            Some(chunk) => buf.extend_from_slice(&chunk.bytes),
            None => break,
        }

        while let Some(msg) = shared::decode_from_buf(&mut buf)? {
            match msg {
                WireMessage::ClientList(ClientList { clients }) => {
                    let _ = event_tx
                        .send(NetworkEvent::ClientListUpdated(clients))
                        .await;
                }
                WireMessage::ClientStatusChanged(ClientStatusChanged {
                    client_id, online, ..
                }) => {
                    info!("Client {} status changed: online={}", client_id, online);
                }
                WireMessage::SessionStarted(SessionStarted {
                    session_id: _,
                    peer,
                }) => {
                    let _ = event_tx
                        .send(NetworkEvent::SessionStarted {
                            client_id: 0,
                            peer_name: peer.node_name,
                        })
                        .await;
                }
                WireMessage::SessionEnded(SessionEnded {
                    session_id: _,
                    reason,
                }) => {
                    let _ = event_tx.send(NetworkEvent::SessionEnded { reason }).await;
                }
                WireMessage::Frame(frame) => {
                    let _ = event_tx
                        .send(NetworkEvent::FrameReceived {
                            width: frame.region.width,
                            height: frame.region.height,
                            data: frame.data.into_vec(),
                        })
                        .await;
                }
                WireMessage::KeepAlive(_) => {}
                _ => {
                    warn!("Unexpected message: {:?}", msg);
                }
            }
        }
    }

    let _ = event_tx.send(NetworkEvent::Disconnected).await;
    Ok(())
}

fn create_client_endpoint() -> Result<Endpoint> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let client_crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(SkipServerVerification))
        .with_no_client_auth();

    let client_config = quinn::ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(client_crypto)?,
    ));

    let mut endpoint = Endpoint::client("0.0.0.0:0".parse()?)?;
    endpoint.set_default_client_config(client_config);

    Ok(endpoint)
}

#[derive(Debug)]
struct SkipServerVerification;

impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::ED25519,
        ]
    }
}

async fn read_message(recv: &mut quinn::RecvStream) -> Result<WireMessage> {
    let mut buf = BytesMut::with_capacity(1024);
    loop {
        if let Some(msg) = shared::decode_from_buf(&mut buf)? {
            return Ok(msg);
        }
        match recv.read_chunk(1024, true).await? {
            Some(chunk) => buf.extend_from_slice(&chunk.bytes),
            None => return Err(anyhow::anyhow!("Stream closed")),
        }
    }
}
