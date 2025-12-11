use std::net::SocketAddr;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub quic_addr: SocketAddr,
    pub auth_token: String,
}

impl ServerConfig {
    pub fn from_env() -> Self {
        Self {
            quic_addr: std::env::var("RELAY_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:4433".to_string())
                .parse()
                .expect("Invalid RELAY_ADDR"),
            auth_token: std::env::var("RELAY_AUTH_TOKEN").expect("RELAY_AUTH_TOKEN must be set"),
        }
    }

    pub fn validate_token(&self, token: &str) -> bool {
        !token.is_empty() && token == self.auth_token
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            quic_addr: "0.0.0.0:4433".parse().unwrap(),
            auth_token: "dev-token".to_string(),
        }
    }
}
