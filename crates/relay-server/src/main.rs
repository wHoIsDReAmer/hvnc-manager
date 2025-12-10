use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use bytes::BytesMut;
use shared::protocol::{Hello, HelloAck, PROTOCOL_VERSION, WireMessage};
use shared::{codec, decode_from_buf, encode_to_vec};
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    // TODO: load from config/CLI
    let quic_addr: SocketAddr = "0.0.0.0:4433".parse()?;
    let tcp_addr: SocketAddr = "0.0.0.0:7443".parse()?;

    // Spawn QUIC and TCP listeners; current TCP path is stub for quick testing.
    let sessions = Arc::new(Mutex::new(SessionMap::default()));

    let quic_task = tokio::spawn(run_quic(quic_addr, Arc::clone(&sessions)));
    let tcp_task = tokio::spawn(run_tcp(tcp_addr, Arc::clone(&sessions)));

    tokio::select! {
        res = quic_task => res??,
        res = tcp_task => res??,
    }

    Ok(())
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .try_init();
}

#[derive(Default)]
struct SessionMap {
    // TODO: manager/client pairing map
}

async fn run_quic(_addr: SocketAddr, _sessions: Arc<Mutex<SessionMap>>) -> Result<()> {
    // Placeholder: QUIC wiring to be implemented next.
    info!("QUIC listener not yet implemented; skipping.");
    Ok(())
}

async fn run_tcp(addr: SocketAddr, sessions: Arc<Mutex<SessionMap>>) -> Result<()> {
    let listener = TcpListener::bind(addr).await?;
    info!("TCP fallback listening on {}", addr);

    loop {
        let (socket, peer) = listener.accept().await?;
        let sessions = Arc::clone(&sessions);
        tokio::spawn(async move {
            if let Err(e) = handle_tcp_conn(socket, peer, sessions).await {
                error!("tcp session error: {e}");
            }
        });
    }
}

async fn handle_tcp_conn(
    mut socket: TcpStream,
    peer: SocketAddr,
    sessions: Arc<Mutex<SessionMap>>,
) -> Result<()> {
    let mut buf = BytesMut::with_capacity(16 * 1024);
    info!("TCP peer connected: {}", peer);

    loop {
        codec::enforce_max_buffer(&mut buf, 1 << 20)?;

        let mut temp = [0u8; 2048];
        let n = socket.read(&mut temp).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&temp[..n]);
        while let Some(msg) = decode_from_buf(&mut buf)? {
            handle_msg(&mut socket, msg, &sessions).await?;
        }
    }
    info!("TCP peer disconnected: {}", peer);
    Ok(())
}

async fn handle_msg(
    socket: &mut TcpStream,
    msg: WireMessage,
    _sessions: &Arc<Mutex<SessionMap>>,
) -> Result<()> {
    match msg {
        WireMessage::Hello(hello) => {
            let ack = validate_hello(&hello);
            let out = encode_to_vec(&WireMessage::HelloAck(ack))?;
            socket.write_all(&out).await?;
        }
        WireMessage::KeepAlive(_) => {
            // echo back for now
            let out = encode_to_vec(&msg)?;
            socket.write_all(&out).await?;
        }
        _ => {
            // ignore other messages in stub
        }
    }
    Ok(())
}

fn validate_hello(hello: &Hello) -> HelloAck {
    if hello.version != PROTOCOL_VERSION {
        return HelloAck {
            accepted: false,
            session_id: None,
            reason: Some("version mismatch".into()),
            negotiated_version: PROTOCOL_VERSION,
        };
    }
    // TODO: auth token verification + session allocation
    HelloAck {
        accepted: true,
        session_id: Some(0),
        reason: None,
        negotiated_version: PROTOCOL_VERSION,
    }
}
