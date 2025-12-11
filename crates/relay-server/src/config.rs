use std::net::SocketAddr;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub quic_addr: SocketAddr,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            quic_addr: "0.0.0.0:4433".parse().unwrap(),
        }
    }
}
