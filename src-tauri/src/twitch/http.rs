//! HTTP client abstraction for Twitch API
//!
//! This module provides a trait-based HTTP client that can be easily mocked for testing.

use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::header::HeaderMap;
use serde::de::DeserializeOwned;

/// Trait for making HTTP requests
///
/// This abstraction allows easy mocking of HTTP calls in tests.
#[async_trait]
pub trait HttpClient: Send + Sync {
    /// Makes a GET request and deserializes the JSON response
    async fn get_json<T: DeserializeOwned + Send>(
        &self,
        url: &str,
        headers: &HeaderMap,
    ) -> Result<T>;

    /// Makes a GET request and returns the raw response for special handling
    async fn get_response(
        &self,
        url: &str,
        headers: &HeaderMap,
    ) -> Result<HttpResponse>;
}

/// Response from an HTTP request
#[derive(Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

impl HttpResponse {
    /// Returns true if status is in 2xx range
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// Returns true if status is 404
    pub fn is_not_found(&self) -> bool {
        self.status == 404
    }

    /// Deserializes the body as JSON
    pub fn json<T: DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_str(&self.body).context("Failed to parse JSON response")
    }
}

/// Production HTTP client using reqwest
#[derive(Debug, Clone)]
pub struct ReqwestClient {
    inner: reqwest::Client,
}

impl ReqwestClient {
    /// Creates a new reqwest-based HTTP client
    pub fn new() -> Self {
        Self {
            inner: reqwest::Client::new(),
        }
    }
}

impl Default for ReqwestClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl HttpClient for ReqwestClient {
    async fn get_json<T: DeserializeOwned + Send>(
        &self,
        url: &str,
        headers: &HeaderMap,
    ) -> Result<T> {
        let response = self
            .inner
            .get(url)
            .headers(headers.clone())
            .send()
            .await
            .context("Failed to send request")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("HTTP error {}: {}", status, body);
        }

        response.json().await.context("Failed to parse response")
    }

    async fn get_response(
        &self,
        url: &str,
        headers: &HeaderMap,
    ) -> Result<HttpResponse> {
        let response = self
            .inner
            .get(url)
            .headers(headers.clone())
            .send()
            .await
            .context("Failed to send request")?;

        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();

        Ok(HttpResponse { status, body })
    }
}

#[cfg(test)]
pub mod mock {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, RwLock};

    /// Mock HTTP client for testing
    ///
    /// Allows setting up canned responses for specific URLs.
    #[derive(Debug, Clone, Default)]
    pub struct MockHttpClient {
        responses: Arc<RwLock<HashMap<String, MockResponse>>>,
        requests: Arc<RwLock<Vec<RecordedRequest>>>,
    }

    /// A recorded HTTP request
    #[derive(Debug, Clone)]
    pub struct RecordedRequest {
        pub url: String,
        pub headers: HeaderMap,
    }

    /// A mock response configuration
    #[derive(Debug, Clone)]
    struct MockResponse {
        status: u16,
        body: String,
    }

    impl MockHttpClient {
        /// Creates a new mock client
        pub fn new() -> Self {
            Self::default()
        }

        /// Configures a response for a URL
        pub fn on_get(self, url: &str, status: u16, body: impl Into<String>) -> Self {
            self.responses
                .write()
                .unwrap()
                .insert(url.to_string(), MockResponse {
                    status,
                    body: body.into(),
                });
            self
        }

        /// Configures a successful JSON response for a URL
        pub fn on_get_json<T: serde::Serialize>(self, url: &str, data: &T) -> Self {
            let body = serde_json::to_string(data).expect("Failed to serialize mock data");
            self.on_get(url, 200, body)
        }

        /// Configures a 404 response for a URL
        pub fn on_get_not_found(self, url: &str) -> Self {
            self.on_get(url, 404, "Not Found")
        }

        /// Returns all recorded requests
        pub fn get_requests(&self) -> Vec<RecordedRequest> {
            self.requests.read().unwrap().clone()
        }

        /// Returns the number of requests made
        pub fn request_count(&self) -> usize {
            self.requests.read().unwrap().len()
        }

        /// Clears all recorded requests
        pub fn clear_requests(&self) {
            self.requests.write().unwrap().clear();
        }
    }

    #[async_trait]
    impl HttpClient for MockHttpClient {
        async fn get_json<T: DeserializeOwned + Send>(
            &self,
            url: &str,
            headers: &HeaderMap,
        ) -> Result<T> {
            // Record the request
            self.requests.write().unwrap().push(RecordedRequest {
                url: url.to_string(),
                headers: headers.clone(),
            });

            // Find matching response
            let responses = self.responses.read().unwrap();
            let mock_response = responses
                .get(url)
                .ok_or_else(|| anyhow::anyhow!("No mock response configured for URL: {}", url))?;

            if mock_response.status >= 400 {
                anyhow::bail!("HTTP error {}: {}", mock_response.status, mock_response.body);
            }

            serde_json::from_str(&mock_response.body)
                .context("Failed to parse mock response")
        }

        async fn get_response(
            &self,
            url: &str,
            headers: &HeaderMap,
        ) -> Result<HttpResponse> {
            // Record the request
            self.requests.write().unwrap().push(RecordedRequest {
                url: url.to_string(),
                headers: headers.clone(),
            });

            // Find matching response
            let responses = self.responses.read().unwrap();
            let mock_response = responses
                .get(url)
                .ok_or_else(|| anyhow::anyhow!("No mock response configured for URL: {}", url))?;

            Ok(HttpResponse {
                status: mock_response.status,
                body: mock_response.body.clone(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::mock::MockHttpClient;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct TestData {
        name: String,
        value: i32,
    }

    #[tokio::test]
    async fn mock_client_returns_configured_json() {
        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        let client = MockHttpClient::new()
            .on_get_json("https://api.example.com/data", &data);

        let result: TestData = client
            .get_json("https://api.example.com/data", &HeaderMap::new())
            .await
            .unwrap();

        assert_eq!(result, data);
    }

    #[tokio::test]
    async fn mock_client_returns_error_for_unknown_url() {
        let client = MockHttpClient::new();

        let result: Result<TestData> = client
            .get_json("https://api.example.com/unknown", &HeaderMap::new())
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No mock response configured"));
    }

    #[tokio::test]
    async fn mock_client_returns_error_for_error_status() {
        let client = MockHttpClient::new()
            .on_get("https://api.example.com/error", 500, "Internal Server Error");

        let result: Result<TestData> = client
            .get_json("https://api.example.com/error", &HeaderMap::new())
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("500"));
    }

    #[tokio::test]
    async fn mock_client_records_requests() {
        let client = MockHttpClient::new()
            .on_get("https://api.example.com/test", 200, "{}");

        let mut headers = HeaderMap::new();
        headers.insert("Authorization", "Bearer token".parse().unwrap());

        let _: serde_json::Value = client
            .get_json("https://api.example.com/test", &headers)
            .await
            .unwrap();

        let requests = client.get_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].url, "https://api.example.com/test");
        assert!(requests[0].headers.contains_key("Authorization"));
    }

    #[tokio::test]
    async fn mock_client_get_response_returns_404() {
        let client = MockHttpClient::new()
            .on_get_not_found("https://api.example.com/missing");

        let response = client
            .get_response("https://api.example.com/missing", &HeaderMap::new())
            .await
            .unwrap();

        assert!(response.is_not_found());
        assert!(!response.is_success());
    }

    #[test]
    fn http_response_is_success() {
        let response = HttpResponse { status: 200, body: "{}".to_string() };
        assert!(response.is_success());

        let response = HttpResponse { status: 201, body: "{}".to_string() };
        assert!(response.is_success());

        let response = HttpResponse { status: 404, body: "{}".to_string() };
        assert!(!response.is_success());

        let response = HttpResponse { status: 500, body: "{}".to_string() };
        assert!(!response.is_success());
    }

    #[test]
    fn http_response_json_parsing() {
        let response = HttpResponse {
            status: 200,
            body: r#"{"name": "test", "value": 42}"#.to_string(),
        };

        let data: TestData = response.json().unwrap();
        assert_eq!(data.name, "test");
        assert_eq!(data.value, 42);
    }
}
