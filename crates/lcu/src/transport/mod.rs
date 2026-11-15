pub mod http;
pub mod tls;
pub(crate) mod ws;

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("TLS setup failed: {0}")]
    Tls(String),
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("WebSocket failed: {0}")]
    Ws(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("timed out: {0}")]
    Timeout(&'static str),
    #[error("unexpected response: {0}")]
    Unexpected(String),
}
