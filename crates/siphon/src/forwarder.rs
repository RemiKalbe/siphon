use anyhow::Result;

/// Forwards incoming tunnel requests to a local service
#[derive(Clone)]
pub struct HttpForwarder {
    local_addr: String,
    client: reqwest::Client,
}

impl HttpForwarder {
    pub fn new(local_addr: String) -> Self {
        Self {
            local_addr,
            client: reqwest::Client::builder()
                .pool_max_idle_per_host(10)
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

    pub fn local_addr(&self) -> &str {
        &self.local_addr
    }

    /// Forward an HTTP request to the local service
    pub async fn forward_http(
        &self,
        method: String,
        uri: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    ) -> Result<(u16, Vec<(String, String)>, Vec<u8>)> {
        // Build the local URL
        let local_url = format!("http://{}{}", self.local_addr, uri);

        tracing::debug!("Forwarding {} {} -> {}", method, uri, local_url);

        // Build request
        let method = reqwest::Method::from_bytes(method.as_bytes())?;
        let mut request = self.client.request(method, &local_url);

        // Add headers (filtering out hop-by-hop headers)
        for (name, value) in headers {
            let name_lower = name.to_lowercase();
            // Skip hop-by-hop headers
            if matches!(
                name_lower.as_str(),
                "host"
                    | "connection"
                    | "keep-alive"
                    | "proxy-authenticate"
                    | "proxy-authorization"
                    | "te"
                    | "trailers"
                    | "transfer-encoding"
                    | "upgrade"
            ) {
                continue;
            }
            request = request.header(&name, &value);
        }

        // Set body
        if !body.is_empty() {
            request = request.body(body);
        }

        // Send request
        let response = request.send().await?;

        // Extract response
        let status = response.status().as_u16();

        let resp_headers: Vec<(String, String)> = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                let name_str = name.to_string();
                let name_lower = name_str.to_lowercase();
                // Skip hop-by-hop headers
                if matches!(
                    name_lower.as_str(),
                    "connection"
                        | "keep-alive"
                        | "proxy-authenticate"
                        | "proxy-authorization"
                        | "te"
                        | "trailers"
                        | "transfer-encoding"
                        | "upgrade"
                ) {
                    return None;
                }
                value.to_str().ok().map(|v| (name_str, v.to_string()))
            })
            .collect();

        let resp_body = response.bytes().await?.to_vec();

        tracing::debug!(
            "Response: {} ({} bytes)",
            status,
            resp_body.len()
        );

        Ok((status, resp_headers, resp_body))
    }
}
