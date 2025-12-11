//! Mock client that sends animated color frames to test the relay-manager connection.
//!
//! Usage:
//!   cargo run --example mock_client -- [relay_addr] [auth_token]
//!
//! Default:
//!   cargo run --example mock_client -- 127.0.0.1:4433 dev-token

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow};
use bytes::BytesMut;
use quinn::Endpoint;
use shared::protocol::{
    FrameFormat, FrameSegment, Hello, HelloAck, InputEvent, PROTOCOL_VERSION, Rect, Role,
    SessionEnded, SessionStarted, WireMessage,
};
use tokio::time::interval;
use tracing::{info, warn};

const WIDTH: u32 = 640;
const HEIGHT: u32 = 480;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let args: Vec<String> = std::env::args().collect();
    let relay_addr = args.get(1).map(|s| s.as_str()).unwrap_or("127.0.0.1:4433");
    let auth_token = args.get(2).map(|s| s.as_str()).unwrap_or("dev-token");

    info!("Mock client starting...");
    info!("Relay: {}, Token: {}", relay_addr, auth_token);

    run_client(relay_addr, auth_token).await
}

async fn run_client(relay_addr: &str, auth_token: &str) -> Result<()> {
    let endpoint = create_client_endpoint()?;
    let connection = endpoint.connect(relay_addr.parse()?, "localhost")?.await?;

    info!("Connected to relay server");

    let (mut send, mut recv) = connection.open_bi().await?;

    // Send Hello
    let hello = WireMessage::Hello(Hello {
        version: PROTOCOL_VERSION,
        role: Role::Client,
        auth_token: auth_token.to_string(),
        node_name: "mock-client".to_string(),
    });
    let bytes = shared::encode_to_vec(&hello)?;
    send.write_all(&bytes).await?;

    // Read HelloAck
    let ack = read_message(&mut recv).await?;
    let client_id = if let WireMessage::HelloAck(HelloAck {
        accepted,
        client_id,
        reason,
        ..
    }) = ack
    {
        if !accepted {
            return Err(anyhow!("Connection rejected: {:?}", reason));
        }
        client_id.unwrap_or(0)
    } else {
        return Err(anyhow!("Expected HelloAck"));
    };

    info!("Registered as client ID: {}", client_id);
    info!("Waiting for manager to connect...");

    // Wait for SessionStarted
    loop {
        let msg = read_message(&mut recv).await?;
        match msg {
            WireMessage::SessionStarted(SessionStarted { session_id, peer }) => {
                info!(
                    "Session {} started with manager: {}",
                    session_id, peer.node_name
                );
                break;
            }
            WireMessage::KeepAlive(_) => {
                // Echo back
                let bytes = shared::encode_to_vec(&msg)?;
                send.write_all(&bytes).await?;
            }
            _ => {
                warn!("Unexpected message while waiting: {:?}", msg);
            }
        }
    }

    // Start sending frames
    info!("Starting frame transmission ({}x{})", WIDTH, HEIGHT);

    let mut frame_interval = interval(Duration::from_millis(33)); // ~30 FPS
    let mut frame_number: u64 = 0;

    // Use a flag to signal session end
    let session_active = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let session_active_for_recv = Arc::clone(&session_active);

    // Spawn receiver task for input events
    let send_clone = Arc::new(tokio::sync::Mutex::new(send));
    let send_for_recv = Arc::clone(&send_clone);

    let recv_task = tokio::spawn(async move {
        let mut buf = BytesMut::with_capacity(4096);
        loop {
            match recv.read_chunk(1024, true).await {
                Ok(Some(chunk)) => {
                    buf.extend_from_slice(&chunk.bytes);
                    while let Ok(Some(msg)) = shared::decode_from_buf(&mut buf) {
                        match msg {
                            WireMessage::Input(input) => {
                                handle_input(input);
                            }
                            WireMessage::SessionEnded(SessionEnded { reason, .. }) => {
                                info!("Session ended: {}", reason);
                                session_active_for_recv
                                    .store(false, std::sync::atomic::Ordering::Relaxed);
                                return;
                            }
                            WireMessage::KeepAlive(_) => {
                                if let Ok(bytes) = shared::encode_to_vec(&msg) {
                                    let mut guard = send_for_recv.lock().await;
                                    let _ = guard.write_all(&bytes).await;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Ok(None) => {
                    session_active_for_recv.store(false, std::sync::atomic::Ordering::Relaxed);
                    break;
                }
                Err(e) => {
                    warn!("Recv error: {}", e);
                    session_active_for_recv.store(false, std::sync::atomic::Ordering::Relaxed);
                    break;
                }
            }
        }
    });

    // Frame sending loop
    while session_active.load(std::sync::atomic::Ordering::Relaxed) {
        frame_interval.tick().await;

        if !session_active.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }

        let frame_data = generate_frame(frame_number, WIDTH, HEIGHT);
        let frame = WireMessage::Frame(FrameSegment {
            sequence: frame_number,
            format: FrameFormat::Rgba8888,
            region: Rect {
                x: 0,
                y: 0,
                width: WIDTH,
                height: HEIGHT,
            },
            data: serde_bytes::ByteBuf::from(frame_data),
        });

        let bytes = shared::encode_to_vec(&frame)?;
        {
            let mut guard = send_clone.lock().await;
            if guard.write_all(&bytes).await.is_err() {
                break;
            }
        }

        frame_number += 1;

        if frame_number % 30 == 0 {
            info!("Sent {} frames", frame_number);
        }
    }

    recv_task.abort();
    info!("Mock client stopped");
    Ok(())
}

fn generate_frame(frame_number: u64, width: u32, height: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity((width * height * 4) as usize);

    let time = frame_number as f32 * 0.05;

    for y in 0..height {
        for x in 0..width {
            let fx = x as f32 / width as f32;
            let fy = y as f32 / height as f32;

            // Animated gradient
            let r = ((fx * 255.0 + time * 50.0) as u32 % 256) as u8;
            let g = ((fy * 255.0 + time * 30.0) as u32 % 256) as u8;
            let b = (((fx + fy) * 127.5 + time * 70.0) as u32 % 256) as u8;

            data.push(r);
            data.push(g);
            data.push(b);
            data.push(255); // Alpha
        }
    }

    data
}

fn handle_input(input: InputEvent) {
    match input {
        InputEvent::Keyboard(kb) => {
            info!("Keyboard: scancode={}, action={:?}", kb.scancode, kb.action);
        }
        InputEvent::Mouse(mouse) => {
            info!("Mouse: {:?}", mouse);
        }
    }
}

async fn read_message(recv: &mut quinn::RecvStream) -> Result<WireMessage> {
    let mut buf = BytesMut::with_capacity(4096);
    loop {
        if let Some(msg) = shared::decode_from_buf(&mut buf)? {
            return Ok(msg);
        }
        match recv.read_chunk(1024, true).await? {
            Some(chunk) => buf.extend_from_slice(&chunk.bytes),
            None => return Err(anyhow!("Stream closed")),
        }
    }
}

fn create_client_endpoint() -> Result<Endpoint> {
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
