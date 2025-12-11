use anyhow::Result;
use quinn::{Connection, RecvStream, SendStream};
use shared::protocol::{
    ClientId, ClientList, ClientStatusChanged, ConnectRequest, DisconnectRequest, ErrorCode,
    PeerInfo, SessionEnded, SessionStarted, WireMessage,
};
use shared::{LinkSide, encode_datagram, encode_to_vec};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::AsyncWriteExt;
use tracing::{debug, info, warn};

use crate::session::SessionManager;

#[derive(Debug, Clone, Copy)]
pub struct PeerId {
    pub side: LinkSide,
    pub id: u64,
}

#[derive(Debug)]
pub struct PeerHandle {
    side: LinkSide,
    id: AtomicU64,
    pub conn: Connection,
    pub control_tx: tokio::sync::Mutex<SendStream>,
}

impl PeerHandle {
    pub fn new(peer_id: PeerId, conn: Connection, control_tx: SendStream) -> Arc<Self> {
        Arc::new(Self {
            side: peer_id.side,
            id: AtomicU64::new(peer_id.id),
            conn,
            control_tx: tokio::sync::Mutex::new(control_tx),
        })
    }

    pub fn get_peer_id(&self) -> PeerId {
        PeerId {
            side: self.side,
            id: self.id.load(Ordering::Relaxed),
        }
    }

    pub fn set_peer_id(&self, peer_id: PeerId) {
        debug_assert_eq!(self.side, peer_id.side);
        self.id.store(peer_id.id, Ordering::Relaxed);
    }

    pub async fn send_control(&self, msg: &WireMessage) -> Result<()> {
        let mut guard = self.control_tx.lock().await;
        let bytes = encode_to_vec(msg)?;
        guard.write_all(&bytes).await?;
        guard.flush().await?; // Ensure data is sent immediately
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn send_datagram(&self, msg: &WireMessage) -> Result<()> {
        let bytes = encode_datagram(msg)?;
        self.conn.send_datagram(bytes.into())?;
        Ok(())
    }

    pub fn send_datagram_raw(&self, data: bytes::Bytes) -> Result<()> {
        self.conn.send_datagram(data)?;
        Ok(())
    }
}

pub async fn control_loop(
    mut recv: RecvStream,
    peer: Arc<PeerHandle>,
    sessions: Arc<SessionManager>,
) -> Result<()> {
    let mut buf = bytes::BytesMut::with_capacity(16 * 1024);
    let peer_id = peer.get_peer_id();
    debug!("control_loop started for {:?}", peer_id);

    loop {
        let chunk = match recv.read_chunk(2048, true).await {
            Ok(Some(c)) => c.bytes,
            Ok(None) => {
                debug!("control_loop: stream ended (EOF) for {:?}", peer_id);
                break;
            }
            Err(e) => {
                warn!("control_loop: read error for {:?}: {}", peer_id, e);
                break;
            }
        };
        buf.extend_from_slice(&chunk);

        while let Some(msg) = shared::decode_from_buf(&mut buf)? {
            debug!(
                "control_loop: received message {:?} from {:?}",
                std::mem::discriminant(&msg),
                peer_id
            );
            handle_message(&peer, &sessions, msg).await?;
        }
    }

    cleanup_peer(&peer, &sessions).await;
    Ok(())
}

async fn handle_message(
    peer: &Arc<PeerHandle>,
    sessions: &Arc<SessionManager>,
    msg: WireMessage,
) -> Result<()> {
    let peer_id = peer.get_peer_id();

    match msg {
        WireMessage::KeepAlive(_) => {
            peer.send_control(&msg).await?;
        }

        WireMessage::Connect(ConnectRequest { target_client_id }) => {
            if peer_id.side != LinkSide::Manager {
                warn!("Connect request from non-manager");
                return Ok(());
            }

            let manager_id = peer_id.id;
            match sessions.connect(manager_id, target_client_id).await {
                Ok((session_id, client_peer, client_name)) => {
                    let manager_msg = WireMessage::SessionStarted(SessionStarted {
                        session_id,
                        peer: PeerInfo {
                            node_name: client_name,
                        },
                    });
                    peer.send_control(&manager_msg).await?;

                    let manager_name = sessions
                        .get_manager_name(manager_id)
                        .await
                        .unwrap_or_default();
                    let client_msg = WireMessage::SessionStarted(SessionStarted {
                        session_id,
                        peer: PeerInfo {
                            node_name: manager_name,
                        },
                    });
                    client_peer.send_control(&client_msg).await?;

                    info!(
                        "Session {} started: manager {} -> client {}",
                        session_id, manager_id, target_client_id
                    );
                    broadcast_client_status_update(sessions, target_client_id, true).await;
                }
                Err(e) => {
                    warn!("Connect failed: {:?}", e);
                    let err_msg = WireMessage::Error {
                        code: ErrorCode::Busy,
                        message: Some(format!("{:?}", e)),
                    };
                    peer.send_control(&err_msg).await?;
                }
            }
        }

        WireMessage::Disconnect(DisconnectRequest { reason }) => {
            if peer_id.side != LinkSide::Manager {
                warn!("Disconnect request from non-manager");
                return Ok(());
            }

            let manager_id = peer_id.id;
            match sessions.disconnect(manager_id).await {
                Ok((client_id, client_peer)) => {
                    let end_msg = WireMessage::SessionEnded(SessionEnded {
                        session_id: 0,
                        reason: reason.unwrap_or_else(|| "Manager disconnected".to_string()),
                    });
                    client_peer.send_control(&end_msg).await?;
                    info!(
                        "Manager {} disconnected from client {}",
                        manager_id, client_id
                    );
                    broadcast_client_status_update(sessions, client_id, true).await;
                }
                Err(e) => {
                    debug!("Disconnect not needed: {:?}", e);
                }
            }
        }

        WireMessage::Input(_) | WireMessage::Frame(_) | WireMessage::FrameReady { .. } => {
            if let Some(counterpart) = sessions
                .get_session_counterpart(peer_id.side, peer_id.id)
                .await
                && let Err(e) = counterpart.send_control(&msg).await
            {
                warn!("Failed to forward to counterpart: {e}");
            }
        }

        WireMessage::Error { code, message } => {
            warn!("Received error from peer: {:?} - {:?}", code, message);
        }

        _ => {
            debug!("Unexpected message type in control loop: {:?}", msg);
        }
    }

    Ok(())
}

async fn cleanup_peer(peer: &Arc<PeerHandle>, sessions: &Arc<SessionManager>) {
    let peer_id = peer.get_peer_id();

    match peer_id.side {
        LinkSide::Client => {
            let client_id = peer_id.id;
            if sessions.unregister_client(client_id).await.is_some() {
                info!("Client {} disconnected (was in session)", client_id);
            } else {
                info!("Client {} disconnected", client_id);
            }
            broadcast_client_offline(sessions, client_id).await;
        }
        LinkSide::Manager => {
            let manager_id = peer_id.id;
            if sessions.unregister_manager(manager_id).await.is_some() {
                info!("Manager {} disconnected (was in session)", manager_id);
            } else {
                info!("Manager {} disconnected", manager_id);
            }
        }
    }
}

async fn broadcast_client_offline(sessions: &Arc<SessionManager>, client_id: ClientId) {
    let msg = WireMessage::ClientStatusChanged(ClientStatusChanged {
        client_id,
        online: false,
        info: None,
    });

    for manager_peer in sessions.get_all_manager_peers().await {
        let _ = manager_peer.send_control(&msg).await;
    }
}

async fn broadcast_client_status_update(
    sessions: &Arc<SessionManager>,
    client_id: ClientId,
    online: bool,
) {
    let info = sessions.get_client(client_id).await;
    let msg = WireMessage::ClientStatusChanged(ClientStatusChanged {
        client_id,
        online,
        info,
    });

    for manager_peer in sessions.get_all_manager_peers().await {
        let _ = manager_peer.send_control(&msg).await;
    }
}

pub async fn send_client_list(
    peer: &Arc<PeerHandle>,
    sessions: &Arc<SessionManager>,
) -> Result<()> {
    let clients = sessions.list_clients().await;
    let msg = WireMessage::ClientList(ClientList { clients });
    peer.send_control(&msg).await
}

pub async fn broadcast_client_online(sessions: &Arc<SessionManager>, client_id: ClientId) {
    let info = sessions.get_client(client_id).await;
    let msg = WireMessage::ClientStatusChanged(ClientStatusChanged {
        client_id,
        online: true,
        info,
    });

    for manager_peer in sessions.get_all_manager_peers().await {
        let _ = manager_peer.send_control(&msg).await;
    }
}
