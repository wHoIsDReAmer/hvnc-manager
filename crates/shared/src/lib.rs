pub mod codec;
pub mod protocol;

pub use codec::*;
pub use protocol::*;

/// Session Types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkSide {
    Manager,
    Client,
}
