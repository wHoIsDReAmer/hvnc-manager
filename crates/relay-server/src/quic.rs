use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use quinn::Endpoint;
use shared::LinkSide;
use shared::protocol::{Hello, HelloAck, PROTOCOL_VERSION, Role, WireMessage};
use tokio::task::JoinSet;
use tracing::{error, info, warn};

use crate::config::ServerConfig;
use crate::session::SessionManager;
use crate::transport::{
    PeerHandle, PeerId, broadcast_client_online, control_loop, send_client_list,
};

pub async fn run_quic(cfg: ServerConfig, sessions: Arc<SessionManager>) -> Result<()> {
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

async fn handle_connection(incoming: quinn::Incoming, sessions: Arc<SessionManager>) -> Result<()> {
    let connection = incoming.await?;
    let remote = connection.remote_address();
    info!("QUIC peer connected: {}", remote);

    let (send, mut recv) = connection
        .accept_bi()
        .await
        .context("accept control bi stream")?;
    let hello = read_hello(&mut recv).await?;

    if hello.version != PROTOCOL_VERSION {
        let ack = HelloAck {
            accepted: false,
            client_id: None,
            reason: Some(format!(
                "Version mismatch: expected {}, got {}",
                PROTOCOL_VERSION, hello.version
            )),
            negotiated_version: PROTOCOL_VERSION,
        };
        let bytes = shared::encode_to_vec(&WireMessage::HelloAck(ack))?;
        let mut send = send;
        send.write_all(&bytes).await?;
        return Ok(());
    }

    if hello.auth_token.is_empty() {
        let ack = HelloAck {
            accepted: false,
            client_id: None,
            reason: Some("Authentication required".to_string()),
            negotiated_version: PROTOCOL_VERSION,
        };
        let bytes = shared::encode_to_vec(&WireMessage::HelloAck(ack))?;
        let mut send = send;
        send.write_all(&bytes).await?;
        return Ok(());
    }

    match hello.role {
        Role::Client => handle_client_connection(connection, send, recv, hello, sessions).await,
        Role::Manager => handle_manager_connection(connection, send, recv, hello, sessions).await,
        Role::Relay => {
            warn!("Relay role not accepted from peer");
            Ok(())
        }
    }
}

async fn handle_client_connection(
    connection: quinn::Connection,
    send: quinn::SendStream,
    recv: quinn::RecvStream,
    hello: Hello,
    sessions: Arc<SessionManager>,
) -> Result<()> {
    let peer = PeerHandle::new(
        PeerId {
            side: LinkSide::Client,
            id: 0,
        },
        connection.clone(),
        send,
    );
    let client_id = sessions
        .register_client(hello.node_name.clone(), Arc::clone(&peer))
        .await;
    peer.set_peer_id(PeerId {
        side: LinkSide::Client,
        id: client_id,
    });

    let ack = HelloAck {
        accepted: true,
        client_id: Some(client_id),
        reason: None,
        negotiated_version: PROTOCOL_VERSION,
    };
    peer.send_control(&WireMessage::HelloAck(ack)).await?;

    info!(
        "Client '{}' registered with id {} from {}",
        hello.node_name,
        client_id,
        connection.remote_address()
    );
    broadcast_client_online(&sessions, client_id).await;

    let sessions_dg = Arc::clone(&sessions);
    let peer_for_dg = Arc::clone(&peer);
    tokio::spawn(async move {
        loop {
            match connection.read_datagram().await {
                Ok(data) => {
                    let peer_id = peer_for_dg.get_peer_id();
                    if let Some(counterpart) = sessions_dg
                        .get_session_counterpart(peer_id.side, peer_id.id)
                        .await
                        && let Err(e) = counterpart.send_datagram_raw(data)
                    {
                        warn!("Failed to forward datagram: {e}");
                    }
                }
                Err(e) => {
                    warn!("Client datagram read ended: {e}");
                    break;
                }
            }
        }
    });

    control_loop(recv, peer, sessions).await
}

async fn handle_manager_connection(
    connection: quinn::Connection,
    send: quinn::SendStream,
    recv: quinn::RecvStream,
    hello: Hello,
    sessions: Arc<SessionManager>,
) -> Result<()> {
    let peer = PeerHandle::new(
        PeerId {
            side: LinkSide::Manager,
            id: 0,
        },
        connection.clone(),
        send,
    );
    let manager_id = sessions
        .register_manager(hello.node_name.clone(), Arc::clone(&peer))
        .await;
    peer.set_peer_id(PeerId {
        side: LinkSide::Manager,
        id: manager_id,
    });

    let ack = HelloAck {
        accepted: true,
        client_id: None,
        reason: None,
        negotiated_version: PROTOCOL_VERSION,
    };
    peer.send_control(&WireMessage::HelloAck(ack)).await?;

    info!(
        "Manager '{}' registered with id {} from {}",
        hello.node_name,
        manager_id,
        connection.remote_address()
    );
    send_client_list(&peer, &sessions).await?;

    let sessions_dg = Arc::clone(&sessions);
    let peer_for_dg = Arc::clone(&peer);
    tokio::spawn(async move {
        loop {
            match connection.read_datagram().await {
                Ok(data) => {
                    let peer_id = peer_for_dg.get_peer_id();
                    if let Some(counterpart) = sessions_dg
                        .get_session_counterpart(peer_id.side, peer_id.id)
                        .await
                        && let Err(e) = counterpart.send_datagram_raw(data)
                    {
                        warn!("Failed to forward datagram: {e}");
                    }
                }
                Err(e) => {
                    warn!("Manager datagram read ended: {e}");
                    break;
                }
            }
        }
    });

    control_loop(recv, peer, sessions).await
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
