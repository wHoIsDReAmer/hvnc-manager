use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;

use super::types::{FrameFormat, Rect};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrameSegment {
    pub sequence: u64,
    pub format: FrameFormat,
    pub region: Rect,
    pub data: ByteBuf,
}
