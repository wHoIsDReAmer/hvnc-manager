use serde::{Deserialize, Serialize};

use super::types::TimestampMs;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeepAlive {
    pub now_ms: TimestampMs,
}
