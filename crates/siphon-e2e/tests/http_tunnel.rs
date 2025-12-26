//! HTTP tunnel end-to-end tests

use hyper::StatusCode;
use siphon_e2e::{MockHttpService, TestClient, TestServer};
use siphon_protocol::TunnelType;

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

#[tokio::test]
async fn test_http_tunnel_basic_get() {
    init_test();

    // 1. Start test server (with mock DNS, generated certs)
    let server = TestServer::start().await;

    // 2. Start mock local service
    let mock = MockHttpService::start().await;
    mock.set_response_body(b"Hello from local service!".to_vec());

    // 3. Connect client and establish tunnel
    let client = TestClient::connect(&server, &mock.addr_string(), None, TunnelType::Http)
        .await
        .expect("Failed to connect client");

    let subdomain = client.subdomain.clone().expect("No subdomain assigned");
    tracing::info!("Tunnel established with subdomain: {}", subdomain);

    // Give the tunnel a moment to fully establish
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // 4. Make HTTP request to HTTP plane with correct Host header
    let http_client = reqwest::Client::new();
    let resp = http_client
        .get(format!("http://{}/test-path", server.http_addr))
        .header("Host", server.host_for(&subdomain))
        .send()
        .await
        .expect("HTTP request failed");

    // 5. Assert response
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.expect("Failed to read body");
    assert_eq!(body, "Hello from local service!");

    // 6. Verify mock received the request
    let requests = mock.get_requests();
    assert_eq!(
        requests.len(),
        1,
        "Expected 1 request, got {}",
        requests.len()
    );
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].uri, "/test-path");

    // 7. Verify DNS record was created
    assert!(
        server.dns_provider.has_record(&subdomain),
        "DNS record should exist for {}",
        subdomain
    );
}

#[tokio::test]
async fn test_http_tunnel_post_with_body() {
    init_test();

    let server = TestServer::start().await;
    let mock = MockHttpService::start().await;
    mock.set_response_status(StatusCode::CREATED);
    mock.set_response_body(br#"{"id": 123, "status": "created"}"#.to_vec());
    mock.add_response_header("Content-Type", "application/json");

    let client = TestClient::connect(&server, &mock.addr_string(), None, TunnelType::Http)
        .await
        .expect("Failed to connect client");

    let subdomain = client.subdomain.clone().expect("No subdomain assigned");
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Send POST request with JSON body
    let http_client = reqwest::Client::new();
    let resp = http_client
        .post(format!("http://{}/api/users", server.http_addr))
        .header("Host", server.host_for(&subdomain))
        .header("Content-Type", "application/json")
        .body(r#"{"name": "Test User", "email": "test@example.com"}"#)
        .send()
        .await
        .expect("HTTP request failed");

    assert_eq!(resp.status(), 201);

    let body = resp.text().await.expect("Failed to read body");
    assert!(body.contains("created"));

    // Verify request was forwarded correctly
    let requests = mock.get_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].uri, "/api/users");

    let req_body = String::from_utf8_lossy(&requests[0].body);
    assert!(req_body.contains("Test User"));
    assert!(req_body.contains("test@example.com"));
}

#[tokio::test]
async fn test_http_tunnel_multiple_requests() {
    init_test();

    let server = TestServer::start().await;
    let mock = MockHttpService::start().await;
    mock.set_response_body(b"OK".to_vec());

    let client = TestClient::connect(&server, &mock.addr_string(), None, TunnelType::Http)
        .await
        .expect("Failed to connect client");

    let subdomain = client.subdomain.clone().expect("No subdomain assigned");
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let http_client = reqwest::Client::new();

    // Send multiple requests
    for i in 0..5 {
        let resp = http_client
            .get(format!("http://{}/request/{}", server.http_addr, i))
            .header("Host", server.host_for(&subdomain))
            .send()
            .await
            .expect("HTTP request failed");

        assert_eq!(resp.status(), 200);
    }

    // Give time for all requests to be processed
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Verify all requests were forwarded
    let requests = mock.get_requests();
    assert_eq!(
        requests.len(),
        5,
        "Expected 5 requests, got {}",
        requests.len()
    );

    for (i, req) in requests.iter().enumerate() {
        assert_eq!(req.uri, format!("/request/{}", i));
    }
}

#[tokio::test]
async fn test_http_tunnel_with_custom_subdomain() {
    init_test();

    let server = TestServer::start().await;
    let mock = MockHttpService::start().await;
    mock.set_response_body(b"Custom subdomain works!".to_vec());

    // Request a specific subdomain
    let client = TestClient::connect(
        &server,
        &mock.addr_string(),
        Some("my-custom-app".to_string()),
        TunnelType::Http,
    )
    .await
    .expect("Failed to connect client");

    let subdomain = client.subdomain.clone().expect("No subdomain assigned");
    assert_eq!(subdomain, "my-custom-app");

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let http_client = reqwest::Client::new();
    let resp = http_client
        .get(format!("http://{}/", server.http_addr))
        .header("Host", server.host_for(&subdomain))
        .send()
        .await
        .expect("HTTP request failed");

    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), "Custom subdomain works!");
}

#[tokio::test]
async fn test_http_tunnel_preserves_headers() {
    init_test();

    let server = TestServer::start().await;
    let mock = MockHttpService::start().await;
    mock.set_response_body(b"Headers received".to_vec());

    let client = TestClient::connect(&server, &mock.addr_string(), None, TunnelType::Http)
        .await
        .expect("Failed to connect client");

    let subdomain = client.subdomain.clone().expect("No subdomain assigned");
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let http_client = reqwest::Client::new();
    let resp = http_client
        .get(format!("http://{}/headers-test", server.http_addr))
        .header("Host", server.host_for(&subdomain))
        .header("X-Custom-Header", "custom-value")
        .header("Authorization", "Bearer test-token")
        .header("Accept", "application/json")
        .send()
        .await
        .expect("HTTP request failed");

    assert_eq!(resp.status(), 200);

    // Verify headers were forwarded
    let requests = mock.get_requests();
    assert_eq!(requests.len(), 1);

    let headers: std::collections::HashMap<String, String> = requests[0]
        .headers
        .iter()
        .map(|(k, v)| (k.to_lowercase(), v.clone()))
        .collect();

    assert_eq!(
        headers.get("x-custom-header"),
        Some(&"custom-value".to_string())
    );
    assert_eq!(
        headers.get("authorization"),
        Some(&"Bearer test-token".to_string())
    );
}

#[tokio::test]
async fn test_http_tunnel_error_response() {
    init_test();

    let server = TestServer::start().await;
    let mock = MockHttpService::start().await;
    mock.set_response_status(StatusCode::NOT_FOUND);
    mock.set_response_body(b"Resource not found".to_vec());

    let client = TestClient::connect(&server, &mock.addr_string(), None, TunnelType::Http)
        .await
        .expect("Failed to connect client");

    let subdomain = client.subdomain.clone().expect("No subdomain assigned");
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let http_client = reqwest::Client::new();
    let resp = http_client
        .get(format!("http://{}/not-found", server.http_addr))
        .header("Host", server.host_for(&subdomain))
        .send()
        .await
        .expect("HTTP request failed");

    assert_eq!(resp.status(), 404);
    assert_eq!(resp.text().await.unwrap(), "Resource not found");
}

#[tokio::test]
async fn test_multiple_tunnels_isolated() {
    init_test();

    let server = TestServer::start().await;

    // Start two mock services with different responses
    let mock1 = MockHttpService::start().await;
    mock1.set_response_body(b"Response from service 1".to_vec());

    let mock2 = MockHttpService::start().await;
    mock2.set_response_body(b"Response from service 2".to_vec());

    // Connect two clients (keep handles alive to maintain tunnels)
    let _client1 = TestClient::connect(
        &server,
        &mock1.addr_string(),
        Some("app1".to_string()),
        TunnelType::Http,
    )
    .await
    .expect("Failed to connect client1");

    let _client2 = TestClient::connect(
        &server,
        &mock2.addr_string(),
        Some("app2".to_string()),
        TunnelType::Http,
    )
    .await
    .expect("Failed to connect client2");

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let http_client = reqwest::Client::new();

    // Request to app1 should go to mock1
    let resp1 = http_client
        .get(format!("http://{}/", server.http_addr))
        .header("Host", server.host_for("app1"))
        .send()
        .await
        .expect("HTTP request failed");

    assert_eq!(resp1.text().await.unwrap(), "Response from service 1");

    // Request to app2 should go to mock2
    let resp2 = http_client
        .get(format!("http://{}/", server.http_addr))
        .header("Host", server.host_for("app2"))
        .send()
        .await
        .expect("HTTP request failed");

    assert_eq!(resp2.text().await.unwrap(), "Response from service 2");

    // Verify each mock only received its own requests
    assert_eq!(mock1.get_requests().len(), 1);
    assert_eq!(mock2.get_requests().len(), 1);

    // Verify both DNS records were created
    assert_eq!(server.dns_provider.record_count(), 2);
}
