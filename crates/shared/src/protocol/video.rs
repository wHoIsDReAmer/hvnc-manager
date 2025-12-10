use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;

use super::types::{FrameFormat, Rect};

/// A frame delta or full frame segment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrameSegment {
    /// Monotonic increasing sequence per session; 0 can mean "full frame".
    pub sequence: u64,
    pub format: FrameFormat,
    pub region: Rect,
    /// Pixel payload; layout depends on `format`.
    pub data: ByteBuf,
}
