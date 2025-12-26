//! Mock HTTP service for E2E tests
//!
//! This module provides a mock HTTP service that records incoming requests
//! and returns configurable responses. It's used as the "local service"
//! that the tunnel client forwards requests to.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use parking_lot::RwLock;
use tokio::net::TcpListener;

/// A recorded HTTP request for test assertions
#[derive(Clone, Debug)]
pub struct RecordedRequest {
    /// HTTP method (GET, POST, etc.)
    pub method: String,
    /// Request URI path
    pub uri: String,
    /// Request headers
    pub headers: Vec<(String, String)>,
    /// Request body
    pub body: Vec<u8>,
}

/// A mock HTTP service for testing
///
/// This service listens on a local port and records all incoming requests.
/// Responses can be configured via the `set_response_*` methods.
pub struct MockHttpService {
    addr: SocketAddr,
    /// Recorded requests
    requests: Arc<RwLock<Vec<RecordedRequest>>>,
    /// Configurable response status
    response_status: Arc<RwLock<StatusCode>>,
    /// Configurable response body
    response_body: Arc<RwLock<Vec<u8>>>,
    /// Configurable response headers
    response_headers: Arc<RwLock<Vec<(String, String)>>>,
}

impl MockHttpService {
    /// Start a mock HTTP service on an ephemeral port
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind mock service");
        let addr = listener.local_addr().unwrap();

        let requests: Arc<RwLock<Vec<RecordedRequest>>> = Arc::new(RwLock::new(Vec::new()));
        let response_status = Arc::new(RwLock::new(StatusCode::OK));
        let response_body: Arc<RwLock<Vec<u8>>> = Arc::new(RwLock::new(b"OK".to_vec()));
        let response_headers: Arc<RwLock<Vec<(String, String)>>> = Arc::new(RwLock::new(vec![]));

        let requests_clone = requests.clone();
        let status_clone = response_status.clone();
        let body_clone = response_body.clone();
        let headers_clone = response_headers.clone();

        tokio::spawn(async move {
            loop {
                let (stream, _) = match listener.accept().await {
                    Ok(conn) => conn,
                    Err(_) => break,
                };

                let requests = requests_clone.clone();
                let status = status_clone.clone();
                let body = body_clone.clone();
                let headers = headers_clone.clone();

                tokio::spawn(async move {
                    let service = service_fn(move |req: Request<Incoming>| {
                        let requests = requests.clone();
                        let status = status.clone();
                        let body = body.clone();
                        let headers = headers.clone();
                        async move {
                            // Record the request
                            let method = req.method().to_string();
                            let uri = req.uri().to_string();
                            let req_headers: Vec<(String, String)> = req
                                .headers()
                                .iter()
                                .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                                .collect();

                            let req_body = req
                                .into_body()
                                .collect()
                                .await
                                .map(|b| b.to_bytes().to_vec())
                                .unwrap_or_default();

                            requests.write().push(RecordedRequest {
                                method,
                                uri,
                                headers: req_headers,
                                body: req_body,
                            });

                            // Build response
                            let resp_status = *status.read();
                            let resp_body = body.read().clone();
                            let resp_headers = headers.read().clone();

                            let mut builder = Response::builder().status(resp_status);
                            for (name, value) in resp_headers {
                                builder = builder.header(name, value);
                            }

                            Ok::<_, Infallible>(
                                builder
                                    .body(Full::new(Bytes::from(resp_body)))
                                    .unwrap(),
                            )
                        }
                    });

                    let _ = http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service)
                        .await;
                });
            }
        });

        Self {
            addr,
            requests,
            response_status,
            response_body,
            response_headers,
        }
    }

    /// Get the address this service is listening on
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Get the address as a string (e.g., "127.0.0.1:12345")
    pub fn addr_string(&self) -> String {
        self.addr.to_string()
    }

    /// Get the port this service is listening on
    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// Get all recorded requests
    pub fn get_requests(&self) -> Vec<RecordedRequest> {
        self.requests.read().clone()
    }

    /// Get the last recorded request (if any)
    pub fn last_request(&self) -> Option<RecordedRequest> {
        self.requests.read().last().cloned()
    }

    /// Clear recorded requests
    pub fn clear_requests(&self) {
        self.requests.write().clear();
    }

    /// Set the response status code
    pub fn set_response_status(&self, status: StatusCode) {
        *self.response_status.write() = status;
    }

    /// Set the response body
    pub fn set_response_body(&self, body: impl Into<Vec<u8>>) {
        *self.response_body.write() = body.into();
    }

    /// Set response headers
    pub fn set_response_headers(&self, headers: Vec<(String, String)>) {
        *self.response_headers.write() = headers;
    }

    /// Add a single response header
    pub fn add_response_header(&self, name: impl Into<String>, value: impl Into<String>) {
        self.response_headers
            .write()
            .push((name.into(), value.into()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_service_basic() {
        let service = MockHttpService::start().await;
        service.set_response_body(b"Hello, World!".to_vec());

        // Make a request
        let client = reqwest::Client::new();
        let resp = client
            .get(format!("http://{}/test", service.addr()))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), 200);
        assert_eq!(resp.text().await.unwrap(), "Hello, World!");

        // Check recorded request
        let requests = service.get_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "GET");
        assert_eq!(requests[0].uri, "/test");
    }

    #[tokio::test]
    async fn test_mock_service_post() {
        let service = MockHttpService::start().await;
        service.set_response_status(StatusCode::CREATED);
        service.set_response_body(r#"{"id": 1}"#);

        let client = reqwest::Client::new();
        let resp = client
            .post(format!("http://{}/users", service.addr()))
            .body(r#"{"name": "test"}"#)
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), 201);

        let requests = service.get_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "POST");
        assert_eq!(
            String::from_utf8_lossy(&requests[0].body),
            r#"{"name": "test"}"#
        );
    }
}
