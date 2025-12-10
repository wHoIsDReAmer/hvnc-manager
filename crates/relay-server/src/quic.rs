use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use quinn::Endpoint;
use shared::LinkSide;
use shared::protocol::{Hello, HelloAck, PROTOCOL_VERSION, Role, WireMessage};
use tokio::task::JoinSet;
use tracing::{error, info, warn};

use crate::config::ServerConfig;
use crate::session::SessionMap;
use crate::transport::{PeerHandle, control_loop};

pub async fn run_quic(cfg: ServerConfig, sessions: Arc<SessionMap>) -> Result<()> {
    let endpoint = quinn_server(cfg.quic_addr).context("start quic server")?;
    info!("QUIC listening on {}", cfg.quic_addr);

    let mut tasks = JoinSet::new();
    while let Some(incoming) = endpoint.accept().await {
        let sessions = Arc::clone(&sessions);
        tasks.spawn(async move {
            if let Err(e) = handle_connection(incoming, sessions).await {
                error!("quic conn error: {e}");
            }
        });
    }
    while let Some(res) = tasks.join_next().await {
        if let Err(e) = res {
            error!("task failed: {e}");
        }
    }
    drop(endpoint);
    Ok(())
}

async fn handle_connection(incoming: quinn::Incoming, sessions: Arc<SessionMap>) -> Result<()> {
    let connection = incoming.await?;
    let remote = connection.remote_address();
    info!("QUIC peer connected: {}", remote);

    // Control stream = first bi-stream
    let (send, mut recv) = connection
        .accept_bi()
        .await
        .context("accept control bi stream")?;

    let hello = read_hello(&mut recv).await?;
    let side = match hello.role {
        Role::Manager => LinkSide::Manager,
        Role::Client => LinkSide::Client,
        Role::Relay => {
            warn!("Relay role not accepted from peer");
            return Ok(());
        }
    };

    // Register peer
    let peer = PeerHandle::new(side, connection.clone(), send);
    let (session_id, counterpart) = sessions
        .register(hello.auth_token.clone(), side, Arc::clone(&peer))
        .await;

    let ack = build_ack(&hello, session_id);
    peer.send_control(&WireMessage::HelloAck(ack)).await?;

    if let Some(other) = counterpart {
        info!(
            "Paired session {} between {:?} and {:?}",
            session_id, side, other.side
        );
    } else {
        info!("Waiting for counterpart for token");
    }

    // Spawn control-plane message router
    let sessions_ctrl = Arc::clone(&sessions);
    let peer_ctrl = Arc::clone(&peer);
    tokio::spawn(async move {
        if let Err(e) = control_loop(recv, peer_ctrl, session_id, sessions_ctrl).await {
            warn!("control loop error: {e}");
        }
    });

    // Datagram forwarding loop (for video frames)
    let sessions_dg = Arc::clone(&sessions);
    tokio::spawn(async move {
        loop {
            match connection.read_datagram().await {
                Ok(data) => {
                    // Forward raw datagram to counterpart
                    if let Some(counterpart) = sessions_dg.get_counterpart(session_id, side).await
                        && let Err(e) = counterpart.send_datagram_raw(data) {
                            warn!("Failed to forward datagram: {e}");
                        }
                }
                Err(e) => {
                    warn!("datagram read ended: {e}");
                    break;
                }
            }
        }
    });

    Ok(())
}

async fn read_hello(recv: &mut quinn::RecvStream) -> Result<Hello> {
    let mut buf = bytes::BytesMut::with_capacity(1024);
    loop {
        if let Some(msg) = shared::decode_from_buf(&mut buf)? {
            if let WireMessage::Hello(h) = msg {
                return Ok(h);
            } else {
                warn!("First message not Hello; dropping");
                return Err(anyhow!("Expected Hello"));
            }
        }
        match recv.read_chunk(1024, true).await? {
            Some(chunk) => buf.extend_from_slice(&chunk.bytes),
            None => return Err(anyhow!("stream closed before hello")),
        }
    }
}

fn build_ack(hello: &Hello, session_id: u64) -> HelloAck {
    if hello.version != PROTOCOL_VERSION {
        return HelloAck {
            accepted: false,
            session_id: None,
            reason: Some("version mismatch".into()),
            negotiated_version: PROTOCOL_VERSION,
        };
    }
    HelloAck {
        accepted: true,
        session_id: if session_id > 0 {
            Some(session_id)
        } else {
            None
        },
        reason: None,
        negotiated_version: PROTOCOL_VERSION,
    }
}

fn quinn_server(addr: std::net::SocketAddr) -> Result<Endpoint> {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

    let certified_key = rcgen::generate_simple_self_signed(vec!["localhost".into()])?;
    let cert_der = CertificateDer::from(certified_key.cert.der().to_vec());
    let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
        certified_key.key_pair.serialize_der(),
    ));

    let mut server_config = quinn::ServerConfig::with_single_cert(vec![cert_der], key_der)?;
    let mut transport_config = quinn::TransportConfig::default();
    transport_config.keep_alive_interval(Some(Duration::from_secs(10)));
    server_config.transport_config(Arc::new(transport_config));

    Ok(Endpoint::server(server_config, addr)?)
}
