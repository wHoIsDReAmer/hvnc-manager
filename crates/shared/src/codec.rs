use std::io;

use bytes::{Buf, BufMut, BytesMut};

use crate::protocol::message::WireMessage;

const LEN_BYTES: usize = 4;

pub type CodecResult<T> = Result<T, bitcode::Error>;

pub fn encode_to_vec(msg: &WireMessage) -> CodecResult<Vec<u8>> {
    let payload = bitcode::serialize(msg)?;
    let mut out = Vec::with_capacity(LEN_BYTES + payload.len());
    out.put_u32_le(payload.len() as u32);
    out.extend_from_slice(&payload);
    Ok(out)
}

pub fn decode_from_buf(buf: &mut BytesMut) -> CodecResult<Option<WireMessage>> {
    if buf.len() < LEN_BYTES {
        return Ok(None);
    }
    let len = (&buf[..LEN_BYTES]).get_u32_le() as usize;
    if buf.len() < LEN_BYTES + len {
        return Ok(None);
    }
    buf.advance(LEN_BYTES);
    let payload = buf.split_to(len);
    let msg = bitcode::deserialize(&payload)?;
    Ok(Some(msg))
}

pub fn encode_datagram(msg: &WireMessage) -> CodecResult<Vec<u8>> {
    bitcode::serialize(msg)
}

pub fn decode_datagram(bytes: &[u8]) -> CodecResult<WireMessage> {
    bitcode::deserialize(bytes)
}

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
