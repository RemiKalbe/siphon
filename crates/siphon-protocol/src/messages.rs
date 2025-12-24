use serde::{Deserialize, Serialize};

/// Type of tunnel to establish
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TunnelType {
    /// HTTP tunnel (proxied through Cloudflare)
    Http,
    /// Raw TCP tunnel (DNS-only, direct connection)
    Tcp,
}

/// Messages sent from client to server
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Request to establish a tunnel
    RequestTunnel {
        /// Requested subdomain (None = auto-generate)
        subdomain: Option<String>,
        /// Type of tunnel
        tunnel_type: TunnelType,
        /// Local port description (for display purposes)
        local_port: u16,
    },

    /// Response data for an HTTP request
    HttpResponse {
        /// Stream ID this response belongs to
        stream_id: u64,
        /// HTTP status code
        status: u16,
        /// Response headers
        headers: Vec<(String, String)>,
        /// Response body
        body: Vec<u8>,
    },

    /// TCP data from client to server (response to TcpData)
    TcpData {
        /// Stream ID for this TCP connection
        stream_id: u64,
        /// Data bytes
        data: Vec<u8>,
    },

    /// TCP connection closed by local service
    TcpClose {
        /// Stream ID for this TCP connection
        stream_id: u64,
    },

    /// Keepalive ping
    Ping {
        /// Timestamp for RTT measurement
        timestamp: u64,
    },
}

/// Messages sent from server to client
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// Tunnel successfully established
    TunnelEstablished {
        /// Assigned subdomain
        subdomain: String,
        /// Full URL for HTTP tunnels
        url: String,
        /// Assigned port for TCP tunnels (None for HTTP)
        port: Option<u16>,
    },

    /// Tunnel request denied
    TunnelDenied {
        /// Reason for denial
        reason: String,
    },

    /// Incoming HTTP request to forward to local service
    HttpRequest {
        /// Unique stream ID for this request
        stream_id: u64,
        /// HTTP method
        method: String,
        /// Request URI (path + query)
        uri: String,
        /// Request headers
        headers: Vec<(String, String)>,
        /// Request body
        body: Vec<u8>,
    },

    /// New TCP connection established
    TcpConnect {
        /// Stream ID for this TCP connection
        stream_id: u64,
    },

    /// Incoming TCP data
    TcpData {
        /// Stream ID for this TCP connection
        stream_id: u64,
        /// Data bytes
        data: Vec<u8>,
    },

    /// TCP connection closed by remote
    TcpClose {
        /// Stream ID for this TCP connection
        stream_id: u64,
    },

    /// Keepalive pong (response to Ping)
    Pong {
        /// Echo back the timestamp
        timestamp: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_message_serialization() {
        let msg = ClientMessage::RequestTunnel {
            subdomain: Some("myapp".to_string()),
            tunnel_type: TunnelType::Http,
            local_port: 3000,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: ClientMessage = serde_json::from_str(&json).unwrap();

        match parsed {
            ClientMessage::RequestTunnel { subdomain, tunnel_type, local_port } => {
                assert_eq!(subdomain, Some("myapp".to_string()));
                assert_eq!(tunnel_type, TunnelType::Http);
                assert_eq!(local_port, 3000);
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_server_message_serialization() {
        let msg = ServerMessage::TunnelEstablished {
            subdomain: "myapp".to_string(),
            url: "https://myapp.tunnel.example.com".to_string(),
            port: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: ServerMessage = serde_json::from_str(&json).unwrap();

        match parsed {
            ServerMessage::TunnelEstablished { subdomain, url, port } => {
                assert_eq!(subdomain, "myapp");
                assert_eq!(url, "https://myapp.tunnel.example.com");
                assert_eq!(port, None);
            }
            _ => panic!("Wrong variant"),
        }
    }
}
