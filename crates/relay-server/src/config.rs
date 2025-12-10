use std::net::SocketAddr;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub quic_addr: SocketAddr,
    pub max_buf: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            quic_addr: "0.0.0.0:4433".parse().unwrap(),
            max_buf: 1 << 20, // 1 MiB
        }
    }
}
