use bytes::{Buf, BufMut, BytesMut};
use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;
use tokio_util::codec::{Decoder, Encoder};

/// Maximum frame size (16 MB)
const MAX_FRAME_SIZE: usize = 16 * 1024 * 1024;

/// Errors that can occur during encoding/decoding
#[derive(Debug, Error)]
pub enum CodecError {
    #[error("Frame too large: {0} bytes (max {MAX_FRAME_SIZE})")]
    FrameTooLarge(usize),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Length-delimited JSON codec for tunnel messages
///
/// Wire format:
/// ```text
/// +----------------+------------------+
/// | Length (4 bytes| JSON payload     |
/// | big-endian u32)| (variable)       |
/// +----------------+------------------+
/// ```
pub struct TunnelCodec<T> {
    _phantom: std::marker::PhantomData<T>,
}

impl<T> TunnelCodec<T> {
    pub fn new() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T> Default for TunnelCodec<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: DeserializeOwned> Decoder for TunnelCodec<T> {
    type Item = T;
    type Error = CodecError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // Need at least 4 bytes for length prefix
        if src.len() < 4 {
            return Ok(None);
        }

        // Peek at the length without consuming
        let length = u32::from_be_bytes([src[0], src[1], src[2], src[3]]) as usize;

        // Check frame size limit
        if length > MAX_FRAME_SIZE {
            return Err(CodecError::FrameTooLarge(length));
        }

        // Check if we have the full frame
        let total_len = 4 + length;
        if src.len() < total_len {
            // Reserve space for the full frame
            src.reserve(total_len - src.len());
            return Ok(None);
        }

        // Consume the length prefix
        src.advance(4);

        // Take the JSON payload
        let payload = src.split_to(length);

        // Deserialize
        let message = serde_json::from_slice(&payload)?;
        Ok(Some(message))
    }
}

impl<T: Serialize> Encoder<T> for TunnelCodec<T> {
    type Error = CodecError;

    fn encode(&mut self, item: T, dst: &mut BytesMut) -> Result<(), Self::Error> {
        // Serialize to JSON
        let json = serde_json::to_vec(&item)?;

        // Check frame size limit
        if json.len() > MAX_FRAME_SIZE {
            return Err(CodecError::FrameTooLarge(json.len()));
        }

        // Write length prefix
        dst.reserve(4 + json.len());
        dst.put_u32(json.len() as u32);
        dst.put_slice(&json);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::{ClientMessage, ServerMessage, TunnelType};

    #[test]
    fn test_roundtrip_client_message() {
        let mut codec = TunnelCodec::<ClientMessage>::new();
        let msg = ClientMessage::RequestTunnel {
            subdomain: Some("test".to_string()),
            tunnel_type: TunnelType::Http,
            local_port: 8080,
        };

        // Encode
        let mut buf = BytesMut::new();
        codec.encode(msg.clone(), &mut buf).unwrap();

        // Decode
        let decoded = codec.decode(&mut buf).unwrap().unwrap();
        match decoded {
            ClientMessage::RequestTunnel { subdomain, tunnel_type, local_port } => {
                assert_eq!(subdomain, Some("test".to_string()));
                assert_eq!(tunnel_type, TunnelType::Http);
                assert_eq!(local_port, 8080);
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_roundtrip_server_message() {
        let mut codec = TunnelCodec::<ServerMessage>::new();
        let msg = ServerMessage::HttpRequest {
            stream_id: 42,
            method: "GET".to_string(),
            uri: "/api/test".to_string(),
            headers: vec![("Host".to_string(), "example.com".to_string())],
            body: vec![],
        };

        // Encode
        let mut buf = BytesMut::new();
        codec.encode(msg, &mut buf).unwrap();

        // Decode
        let decoded = codec.decode(&mut buf).unwrap().unwrap();
        match decoded {
            ServerMessage::HttpRequest { stream_id, method, uri, .. } => {
                assert_eq!(stream_id, 42);
                assert_eq!(method, "GET");
                assert_eq!(uri, "/api/test");
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_partial_frame() {
        let mut codec = TunnelCodec::<ClientMessage>::new();
        let msg = ClientMessage::Ping { timestamp: 12345 };

        // Encode
        let mut buf = BytesMut::new();
        codec.encode(msg, &mut buf).unwrap();

        // Split the buffer in half
        let full_len = buf.len();
        let mut partial = buf.split_to(full_len / 2);

        // Should return None (incomplete)
        assert!(codec.decode(&mut partial).unwrap().is_none());

        // Add the rest
        partial.unsplit(buf);

        // Now should decode
        let decoded = codec.decode(&mut partial).unwrap().unwrap();
        match decoded {
            ClientMessage::Ping { timestamp } => assert_eq!(timestamp, 12345),
            _ => panic!("Wrong variant"),
        }
    }
}
