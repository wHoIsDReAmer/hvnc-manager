use anyhow::Result;
use quinn::{Connection, RecvStream, SendStream};
use shared::protocol::WireMessage;
use shared::{LinkSide, encode_datagram, encode_to_vec};
use std::sync::Arc;
use tracing::{debug, warn};

use crate::session::{SessionId, SessionMap};

#[derive(Debug)]
pub struct PeerHandle {
    pub side: LinkSide,
    pub conn: Connection,
    pub control_tx: tokio::sync::Mutex<SendStream>,
}

impl PeerHandle {
    pub fn new(side: LinkSide, conn: Connection, control_tx: SendStream) -> Arc<Self> {
        Arc::new(Self {
            side,
            conn,
            control_tx: tokio::sync::Mutex::new(control_tx),
        })
    }

    pub async fn send_control(&self, msg: &WireMessage) -> Result<()> {
        let mut guard = self.control_tx.lock().await;
        let bytes = encode_to_vec(msg)?;
        guard.write_all(&bytes).await?;
        Ok(())
    }

    pub async fn send_datagram(&self, msg: &WireMessage) -> Result<()> {
        let bytes = encode_datagram(msg)?;
        self.conn.send_datagram(bytes.into())?;
        Ok(())
    }

    /// Send raw datagram bytes (for forwarding without re-encoding)
    pub fn send_datagram_raw(&self, data: bytes::Bytes) -> Result<()> {
        self.conn.send_datagram(data)?;
        Ok(())
    }
}

/// Control-plane message router.
/// Routes Input/Frame/FrameReady to counterpart, echoes KeepAlive.
pub async fn control_loop(
    mut recv: RecvStream,
    peer: Arc<PeerHandle>,
    session_id: SessionId,
    sessions: Arc<SessionMap>,
) -> Result<()> {
    let mut buf = bytes::BytesMut::with_capacity(16 * 1024);
    loop {
        let chunk = match recv.read_chunk(2048, true).await? {
            Some(c) => c.bytes,
            None => break,
        };
        buf.extend_from_slice(&chunk);
        while let Some(msg) = shared::decode_from_buf(&mut buf)? {
            match &msg {
                WireMessage::KeepAlive(_) => {
                    // Echo back to same peer
                    peer.send_control(&msg).await?;
                }
                WireMessage::Input(_) | WireMessage::Frame(_) | WireMessage::FrameReady { .. } => {
                    // Forward to counterpart
                    if let Some(counterpart) = sessions.get_counterpart(session_id, peer.side).await
                    {
                        if let Err(e) = counterpart.send_control(&msg).await {
                            warn!("Failed to forward to counterpart: {e}");
                        }
                    } else {
                        debug!("No counterpart for session {session_id}, dropping message");
                    }
                }
                WireMessage::Error { code, message } => {
                    warn!("Received error from peer: {:?} - {:?}", code, message);
                }
                _ => {
                    // Hello/HelloAck shouldn't appear here after handshake
                    debug!("Unexpected message type in control loop: {:?}", msg);
                }
            }
        }
    }
    // Cleanup on disconnect
    sessions.remove_session(session_id).await;
    Ok(())
}
