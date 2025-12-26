//! TCP tunnel end-to-end tests

use siphon_e2e::{MockTcpService, TcpServiceMode, TestClient, TestServer};
use siphon_protocol::TunnelType;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Initialize tracing and crypto provider for tests
fn init_test() {
    // Install rustls crypto provider (ignore if already installed)
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Initialize tracing
    let _ = tracing_subscriber::fmt()
        .with_env_filter("siphon=debug,siphon_e2e=debug")
        .with_test_writer()
        .try_init();
}

/// Helper to read with timeout
async fn read_with_timeout(
    stream: &mut TcpStream,
    buf: &mut [u8],
    timeout: Duration,
) -> Result<usize, String> {
    match tokio::time::timeout(timeout, stream.read(buf)).await {
        Ok(Ok(n)) => Ok(n),
        Ok(Err(e)) => Err(format!("Read error: {}", e)),
        Err(_) => Err("Read timeout".to_string()),
    }
}

#[tokio::test]
async fn test_tcp_tunnel_echo() {
    init_test();

    // 1. Start test server
    let server = TestServer::start().await;

    // 2. Start mock TCP service in echo mode
    let mock = MockTcpService::start().await;

    // 3. Connect client and establish TCP tunnel
    let client = TestClient::connect(&server, &mock.addr_string(), None, TunnelType::Tcp)
        .await
        .expect("Failed to connect client");

    let subdomain = client.subdomain.clone().expect("No subdomain assigned");
    let tcp_port = client.tcp_port.expect("No TCP port assigned");

    tracing::info!(
        "TCP tunnel established: subdomain={}, port={}",
        subdomain,
        tcp_port
    );

    // Give the tunnel time to establish
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 4. Connect to the TCP tunnel port
    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", tcp_port))
        .await
        .expect("Failed to connect to tunnel port");

    // Give time for TcpConnect to propagate
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 5. Send data through the tunnel
    stream
        .write_all(b"Hello through TCP tunnel!")
        .await
        .expect("Failed to write");
    stream.flush().await.expect("Failed to flush");

    // 6. Read echoed response with timeout
    let mut buf = [0u8; 64];
    let n = read_with_timeout(&mut stream, &mut buf, Duration::from_secs(5))
        .await
        .expect("Failed to read echo response");

    assert_eq!(&buf[..n], b"Hello through TCP tunnel!");

    // Close connection
    drop(stream);
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify the mock service received the data
    assert_eq!(mock.connection_count(), 1);
    let connections = mock.get_connections();
    assert_eq!(connections[0].received_data, b"Hello through TCP tunnel!");
}

#[tokio::test]
async fn test_tcp_tunnel_fixed_response() {
    init_test();

    let server = TestServer::start().await;

    // Mock service sends a fixed response
    let mock =
        MockTcpService::start_with_mode(TcpServiceMode::FixedResponse(b"Welcome!".to_vec())).await;

    let client = TestClient::connect(&server, &mock.addr_string(), None, TunnelType::Tcp)
        .await
        .expect("Failed to connect client");

    let tcp_port = client.tcp_port.expect("No TCP port assigned");
    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", tcp_port))
        .await
        .expect("Failed to connect to tunnel port");

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Send request
    stream.write_all(b"Hello").await.expect("Failed to write");
    stream.flush().await.expect("Failed to flush");

    // Read fixed response with timeout
    let mut buf = [0u8; 64];
    let n = read_with_timeout(&mut stream, &mut buf, Duration::from_secs(5))
        .await
        .expect("Failed to read fixed response");

    assert_eq!(&buf[..n], b"Welcome!");
}

#[tokio::test]
async fn test_tcp_tunnel_multiple_connections() {
    init_test();

    let server = TestServer::start().await;
    let mock = MockTcpService::start().await;

    let client = TestClient::connect(&server, &mock.addr_string(), None, TunnelType::Tcp)
        .await
        .expect("Failed to connect client");

    let tcp_port = client.tcp_port.expect("No TCP port assigned");
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Open multiple connections sequentially
    for i in 0..3 {
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", tcp_port))
            .await
            .expect("Failed to connect to tunnel port");

        tokio::time::sleep(Duration::from_millis(50)).await;

        let msg = format!("Message {}", i);
        stream
            .write_all(msg.as_bytes())
            .await
            .expect("Failed to write");
        stream.flush().await.expect("Failed to flush");

        let mut buf = [0u8; 64];
        let n = read_with_timeout(&mut stream, &mut buf, Duration::from_secs(5))
            .await
            .expect("Failed to read response");
        assert_eq!(&buf[..n], msg.as_bytes());
    }

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(mock.connection_count(), 3);
}

#[tokio::test]
async fn test_tcp_tunnel_large_data() {
    init_test();

    let server = TestServer::start().await;
    let mock = MockTcpService::start().await;

    let client = TestClient::connect(&server, &mock.addr_string(), None, TunnelType::Tcp)
        .await
        .expect("Failed to connect client");

    let tcp_port = client.tcp_port.expect("No TCP port assigned");
    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", tcp_port))
        .await
        .expect("Failed to connect to tunnel port");

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Send 64KB of data
    let large_data: Vec<u8> = (0..65536).map(|i| (i % 256) as u8).collect();
    stream
        .write_all(&large_data)
        .await
        .expect("Failed to write large data");
    stream.flush().await.expect("Failed to flush");

    // Read back with timeout
    let mut received = Vec::new();
    let mut buf = [0u8; 8192];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);

    while received.len() < large_data.len() && tokio::time::Instant::now() < deadline {
        match read_with_timeout(&mut stream, &mut buf, Duration::from_millis(500)).await {
            Ok(0) => break,
            Ok(n) => received.extend_from_slice(&buf[..n]),
            Err(_) => break,
        }
    }

    assert_eq!(received.len(), large_data.len(), "Did not receive all data");
    assert_eq!(received, large_data);
}

#[tokio::test]
async fn test_tcp_tunnel_with_custom_subdomain() {
    init_test();

    let server = TestServer::start().await;
    let mock = MockTcpService::start().await;

    let client = TestClient::connect(
        &server,
        &mock.addr_string(),
        Some("my-tcp-service".to_string()),
        TunnelType::Tcp,
    )
    .await
    .expect("Failed to connect client");

    assert_eq!(client.subdomain.as_deref(), Some("my-tcp-service"));
    assert!(client.tcp_port.is_some());

    // Verify DNS record was created
    assert!(server.dns_provider.has_record("my-tcp-service"));
}

#[tokio::test]
async fn test_tcp_tunnel_bidirectional() {
    init_test();

    let server = TestServer::start().await;
    let mock = MockTcpService::start().await;

    let client = TestClient::connect(&server, &mock.addr_string(), None, TunnelType::Tcp)
        .await
        .expect("Failed to connect client");

    let tcp_port = client.tcp_port.expect("No TCP port assigned");
    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", tcp_port))
        .await
        .expect("Failed to connect to tunnel port");

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Send and receive multiple times
    for i in 0..5 {
        let msg = format!("Ping {}", i);
        stream
            .write_all(msg.as_bytes())
            .await
            .expect("Failed to write");
        stream.flush().await.expect("Failed to flush");

        let mut buf = [0u8; 64];
        let n = read_with_timeout(&mut stream, &mut buf, Duration::from_secs(5))
            .await
            .expect("Failed to read response");
        assert_eq!(&buf[..n], msg.as_bytes());
    }
}
