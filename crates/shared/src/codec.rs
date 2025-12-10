use std::io;

use bincode::{DefaultOptions, Options};
use bytes::{Buf, BufMut, BytesMut};

use crate::protocol::message::WireMessage;

const LEN_BYTES: usize = 4;

fn bincode_opts() -> impl Options {
    DefaultOptions::new()
        .with_fixint_encoding()
        .allow_trailing_bytes()
}

/// Encode a message for stream transport (length-prefixed, little-endian u32).
pub fn encode_to_vec(msg: &WireMessage) -> bincode::Result<Vec<u8>> {
    let payload = bincode_opts().serialize(msg)?;
    let mut out = Vec::with_capacity(LEN_BYTES + payload.len());
    out.put_u32_le(payload.len() as u32);
    out.extend_from_slice(&payload);
    Ok(out)
}

/// Try to decode a message from a buffer that may hold partial frames.
/// Returns Ok(Some(msg)) when a full frame is consumed, Ok(None) if not enough data.
pub fn decode_from_buf(buf: &mut BytesMut) -> bincode::Result<Option<WireMessage>> {
    if buf.len() < LEN_BYTES {
        return Ok(None);
    }
    let len = (&buf[..LEN_BYTES]).get_u32_le() as usize;
    if buf.len() < LEN_BYTES + len {
        return Ok(None);
    }
    buf.advance(LEN_BYTES);
    let payload = buf.split_to(len);
    let msg = bincode_opts().deserialize::<WireMessage>(&payload)?;
    Ok(Some(msg))
}

/// Encode for datagram transport (no length prefix).
pub fn encode_datagram(msg: &WireMessage) -> bincode::Result<Vec<u8>> {
    bincode_opts().serialize(msg)
}

/// Decode a single datagram payload.
pub fn decode_datagram(bytes: &[u8]) -> bincode::Result<WireMessage> {
    bincode_opts().deserialize(bytes)
}

/// Drain and discard bytes if the buffer grows too large (simple DoS guard).
pub fn enforce_max_buffer(buf: &mut BytesMut, max_len: usize) -> io::Result<()> {
    if buf.len() > max_len {
        buf.clear();
        return Err(io::Error::other("buffer overflow protection triggered"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Hello;
    use crate::protocol::types::{PROTOCOL_VERSION, Role};

    #[test]
    fn stream_roundtrip() {
        let msg = WireMessage::Hello(Hello {
            version: PROTOCOL_VERSION,
            role: Role::Manager,
            auth_token: "t".into(),
            node_name: "mgr".into(),
        });
        let mut buf = BytesMut::new();
        buf.extend_from_slice(&encode_to_vec(&msg).unwrap());
        let decoded = decode_from_buf(&mut buf).unwrap().unwrap();
        assert_eq!(msg, decoded);
        assert!(buf.is_empty());
    }

    #[test]
    fn datagram_roundtrip() {
        let msg = WireMessage::Hello(Hello {
            version: PROTOCOL_VERSION,
            role: Role::Client,
            auth_token: "x".into(),
            node_name: "cli".into(),
        });
        let bytes = encode_datagram(&msg).unwrap();
        let decoded = decode_datagram(&bytes).unwrap();
        assert_eq!(msg, decoded);
    }
}
