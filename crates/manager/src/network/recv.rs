use anyhow::Result;
use bytes::BytesMut;
use quinn::RecvStream;
use shared::protocol::{
    ClientList, ClientStatusChanged, SessionEnded, SessionStarted, WireMessage,
};
use tokio::sync::mpsc;
use tracing::{info, warn};

use super::event::NetworkEvent;

pub async fn receive_loop_with_buf(
    mut recv: RecvStream,
    event_tx: mpsc::Sender<NetworkEvent>,
    mut buf: BytesMut,
) -> Result<()> {
    // First, process any remaining data in the buffer
    while let Some(msg) = shared::decode_from_buf(&mut buf)? {
        handle_receive_message(&event_tx, msg).await;
    }

    // Then continue reading from the stream
    loop {
        match recv.read_chunk(8192, true).await? {
            Some(chunk) => buf.extend_from_slice(&chunk.bytes),
            None => break,
        }

        while let Some(msg) = shared::decode_from_buf(&mut buf)? {
            handle_receive_message(&event_tx, msg).await;
        }
    }

    let _ = event_tx.send(NetworkEvent::Disconnected).await;
    Ok(())
}

async fn handle_receive_message(event_tx: &mpsc::Sender<NetworkEvent>, msg: WireMessage) {
    match msg {
        WireMessage::ClientList(ClientList { clients }) => {
            let _ = event_tx
                .send(NetworkEvent::ClientListUpdated(clients))
                .await;
        }
        WireMessage::ClientStatusChanged(ClientStatusChanged {
            client_id,
            online,
            info,
        }) => {
            info!("Client {} status changed: online={}", client_id, online);
            let _ = event_tx
                .send(NetworkEvent::ClientStatusChanged {
                    client_id,
                    online,
                    info,
                })
                .await;
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

pub async fn read_message_with_buf(
    recv: &mut quinn::RecvStream,
    buf: &mut BytesMut,
) -> Result<WireMessage> {
    loop {
        if let Some(msg) = shared::decode_from_buf(buf)? {
            return Ok(msg);
        }
        match recv.read_chunk(1024, true).await? {
            Some(chunk) => buf.extend_from_slice(&chunk.bytes),
            None => return Err(anyhow::anyhow!("Stream closed")),
        }
    }
}
