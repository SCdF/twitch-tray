use anyhow::{Context, Result};
use reqwest::header::HeaderMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::http::{HttpClient, ReqwestClient};
use super::types::*;
use super::ApiError;

const HELIX_BASE_URL: &str = "https://api.twitch.tv/helix";

/// Twitch Helix API client
///
/// Generic over the HTTP client implementation for testability.
pub struct TwitchClient<H: HttpClient = ReqwestClient> {
    http: H,
    client_id: String,
    access_token: Arc<RwLock<Option<String>>>,
    user_id: Arc<RwLock<Option<String>>>,
}

impl TwitchClient<ReqwestClient> {
    /// Creates a new Twitch API client with the default HTTP implementation
    pub fn new(client_id: String) -> Self {
        Self {
            http: ReqwestClient::new(),
            client_id,
            access_token: Arc::new(RwLock::new(None)),
            user_id: Arc::new(RwLock::new(None)),
        }
    }
}

impl<H: HttpClient> TwitchClient<H> {
    /// Sets the access token for API requests
    pub async fn set_access_token(&self, token: String) {
        let mut guard = self.access_token.write().await;
        *guard = Some(token);
    }

    /// Sets the authenticated user's ID
    pub async fn set_user_id(&self, user_id: String) {
        let mut guard = self.user_id.write().await;
        *guard = Some(user_id);
    }

    /// Gets the current access token
    pub async fn get_access_token(&self) -> Option<String> {
        self.access_token.read().await.clone()
    }

    /// Gets the authenticated user's ID
    pub async fn get_user_id(&self) -> Option<String> {
        self.user_id.read().await.clone()
    }

    /// Builds the headers for an authenticated request
    async fn build_headers(&self) -> Result<HeaderMap> {
        let token = self
            .access_token
            .read()
            .await
            .clone()
            .context("No access token set")?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "Authorization",
            format!("Bearer {}", token).parse().unwrap(),
        );
        headers.insert("Client-Id", self.client_id.parse().unwrap());

        Ok(headers)
    }

    /// Makes an authenticated GET request to the Helix API
    ///
    /// Returns `ApiError::Unauthorized` for 401 responses, allowing callers
    /// to handle token refresh and retry.
    async fn get<T: serde::de::DeserializeOwned + Send>(
        &self,
        endpoint: &str,
    ) -> Result<T, ApiError> {
        let headers = self.build_headers().await?;
        let url = format!("{}{}", HELIX_BASE_URL, endpoint);

        let response = self.http.get_response(&url, &headers).await?;

        if response.is_unauthorized() {
            return Err(ApiError::Unauthorized);
        }

        if !response.is_success() {
            return Err(ApiError::Other(anyhow::anyhow!(
                "API error {}: {}",
                response.status,
                response.body
            )));
        }

        Ok(response.json()?)
    }

    /// Makes an authenticated GET request that may return 404
    ///
    /// Returns `ApiError::Unauthorized` for 401 responses, allowing callers
    /// to handle token refresh and retry. Returns `Ok(None)` for 404.
    async fn get_optional<T: serde::de::DeserializeOwned + Send>(
        &self,
        endpoint: &str,
    ) -> Result<Option<T>, ApiError> {
        let headers = self.build_headers().await?;
        let url = format!("{}{}", HELIX_BASE_URL, endpoint);

        let response = self.http.get_response(&url, &headers).await?;

        if response.is_unauthorized() {
            return Err(ApiError::Unauthorized);
        }

        if response.is_not_found() {
            return Ok(None);
        }

        if !response.is_success() {
            tracing::warn!("API error {}: {}", response.status, response.body);
            return Ok(None);
        }

        Ok(Some(response.json()?))
    }

    /// Clears authentication state
    pub async fn clear_auth(&self) {
        *self.access_token.write().await = None;
        *self.user_id.write().await = None;
    }
}

impl<H: HttpClient + Clone> Clone for TwitchClient<H> {
    fn clone(&self) -> Self {
        Self {
            http: self.http.clone(),
            client_id: self.client_id.clone(),
            access_token: self.access_token.clone(),
            user_id: self.user_id.clone(),
        }
    }
}

// Stream-related methods
impl<H: HttpClient> TwitchClient<H> {
    /// Gets live streams from channels the user follows
    ///
    /// Returns `ApiError::Unauthorized` if the token has expired.
    pub async fn get_followed_streams(&self) -> Result<Vec<Stream>, ApiError> {
        let user_id = self
            .get_user_id()
            .await
            .context("User ID not set")
            .map_err(ApiError::Other)?;

        let mut all_streams = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let endpoint = match &cursor {
                Some(c) => format!(
                    "/streams/followed?user_id={}&first=100&after={}",
                    user_id, c
                ),
                None => format!("/streams/followed?user_id={}&first=100", user_id),
            };

            let response: StreamsResponse = self.get(&endpoint).await?;
            all_streams.extend(response.data);

            match response.pagination.and_then(|p| p.cursor) {
                Some(c) if !c.is_empty() => cursor = Some(c),
                _ => break,
            }
        }

        Ok(all_streams)
    }
}

// Channel-related methods
impl<H: HttpClient> TwitchClient<H> {
    /// Gets channels the user follows (paginated)
    ///
    /// Returns `ApiError::Unauthorized` if the token has expired.
    pub async fn get_followed_channels(
        &self,
        cursor: Option<&str>,
    ) -> Result<(Vec<FollowedChannel>, Option<String>), ApiError> {
        let user_id = self
            .get_user_id()
            .await
            .context("User ID not set")
            .map_err(ApiError::Other)?;

        let endpoint = match cursor {
            Some(c) => format!(
                "/channels/followed?user_id={}&first=100&after={}",
                user_id, c
            ),
            None => format!("/channels/followed?user_id={}&first=100", user_id),
        };

        let response: FollowedChannelsResponse = self.get(&endpoint).await?;
        let next_cursor = response.pagination.and_then(|p| p.cursor);

        Ok((response.data, next_cursor))
    }

    /// Gets all channels the user follows (handles pagination)
    ///
    /// Returns `ApiError::Unauthorized` if the token has expired.
    pub async fn get_all_followed_channels(&self) -> Result<Vec<FollowedChannel>, ApiError> {
        let mut all_follows = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let (follows, next_cursor) = self.get_followed_channels(cursor.as_deref()).await?;
            all_follows.extend(follows);

            match next_cursor {
                Some(c) if !c.is_empty() => cursor = Some(c),
                _ => break,
            }
        }

        Ok(all_follows)
    }
}

// Category-related methods
impl<H: HttpClient> TwitchClient<H> {
    /// Searches for categories/games by name
    ///
    /// Returns `ApiError::Unauthorized` if the token has expired.
    pub async fn search_categories(&self, query: &str) -> Result<Vec<Category>, ApiError> {
        let encoded_query = urlencoding::encode(query);
        let endpoint = format!("/search/categories?query={}&first=10", encoded_query);
        let response: SearchCategoriesResponse = self.get(&endpoint).await?;
        Ok(response.data)
    }

    /// Gets top streams for a specific category/game
    ///
    /// Returns `ApiError::Unauthorized` if the token has expired.
    pub async fn get_streams_by_category(&self, game_id: &str) -> Result<Vec<Stream>, ApiError> {
        let endpoint = format!("/streams?game_id={}&first=10", game_id);
        let response: StreamsResponse = self.get(&endpoint).await?;
        Ok(response.data)
    }
}

// Schedule-related methods
impl<H: HttpClient> TwitchClient<H> {
    /// Gets scheduled streams for a broadcaster
    ///
    /// Returns `ApiError::Unauthorized` if the token has expired.
    /// Returns `Ok(None)` if the broadcaster has no schedule (404).
    pub async fn get_schedule(
        &self,
        broadcaster_id: &str,
    ) -> Result<Option<ScheduleData>, ApiError> {
        let endpoint = format!("/schedule?broadcaster_id={}&first=10", broadcaster_id);

        let response: Option<ScheduleResponse> = self.get_optional(&endpoint).await?;
        Ok(response.map(|r| r.data))
    }
}

/// Test-only constructor for dependency injection
#[cfg(test)]
impl<H: HttpClient> TwitchClient<H> {
    /// Creates a new Twitch API client with a custom HTTP implementation
    pub fn with_http_client(client_id: String, http: H) -> Self {
        Self {
            http,
            client_id,
            access_token: Arc::new(RwLock::new(None)),
            user_id: Arc::new(RwLock::new(None)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::twitch::http::mock::MockHttpClient;
    use chrono::{Duration, Utc};

    fn make_streams_response(streams: Vec<Stream>, cursor: Option<&str>) -> StreamsResponse {
        StreamsResponse {
            data: streams,
            pagination: cursor.map(|c| Pagination {
                cursor: Some(c.to_string()),
            }),
        }
    }

    fn make_stream(user_id: &str, user_name: &str) -> Stream {
        Stream {
            id: format!("stream_{}", user_id),
            user_id: user_id.to_string(),
            user_login: user_name.to_lowercase(),
            user_name: user_name.to_string(),
            game_id: "game123".to_string(),
            game_name: "Test Game".to_string(),
            title: "Test Stream".to_string(),
            viewer_count: 1000,
            started_at: Utc::now() - Duration::hours(1),
            thumbnail_url: "https://example.com/thumb.jpg".to_string(),
            tags: vec![],
        }
    }

    #[tokio::test]
    async fn get_followed_streams_single_page() {
        let streams = vec![
            make_stream("1", "StreamerOne"),
            make_stream("2", "StreamerTwo"),
        ];

        let mock = MockHttpClient::new().on_get_json(
            "https://api.twitch.tv/helix/streams/followed?user_id=user123&first=100",
            &make_streams_response(streams, None),
        );

        let client = TwitchClient::with_http_client("test_client_id".to_string(), mock);
        client.set_access_token("test_token".to_string()).await;
        client.set_user_id("user123".to_string()).await;

        let result = client.get_followed_streams().await.unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].user_id, "1");
        assert_eq!(result[1].user_id, "2");
    }

    #[tokio::test]
    async fn get_followed_streams_pagination() {
        let page1_streams = vec![make_stream("1", "StreamerOne")];
        let page2_streams = vec![make_stream("2", "StreamerTwo")];

        let mock = MockHttpClient::new()
            .on_get_json(
                "https://api.twitch.tv/helix/streams/followed?user_id=user123&first=100",
                &make_streams_response(page1_streams, Some("cursor1")),
            )
            .on_get_json(
                "https://api.twitch.tv/helix/streams/followed?user_id=user123&first=100&after=cursor1",
                &make_streams_response(page2_streams, None),
            );

        let client = TwitchClient::with_http_client("test_client_id".to_string(), mock);
        client.set_access_token("test_token".to_string()).await;
        client.set_user_id("user123".to_string()).await;

        let result = client.get_followed_streams().await.unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].user_id, "1");
        assert_eq!(result[1].user_id, "2");
    }

    #[tokio::test]
    async fn get_followed_streams_requires_user_id() {
        let mock = MockHttpClient::new();
        let client = TwitchClient::with_http_client("test_client_id".to_string(), mock);
        client.set_access_token("test_token".to_string()).await;
        // Don't set user_id

        let result = client.get_followed_streams().await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("User ID not set"));
    }

    #[tokio::test]
    async fn clear_auth_clears_both_token_and_user_id() {
        let mock = MockHttpClient::new();
        let client = TwitchClient::with_http_client("test_client_id".to_string(), mock);

        client.set_access_token("test_token".to_string()).await;
        client.set_user_id("user123".to_string()).await;

        assert!(client.get_user_id().await.is_some());

        client.clear_auth().await;

        assert!(client.get_user_id().await.is_none());
    }

    #[tokio::test]
    async fn client_sends_correct_headers() {
        let streams = vec![make_stream("1", "Streamer")];
        let mock = MockHttpClient::new().on_get_json(
            "https://api.twitch.tv/helix/streams/followed?user_id=user123&first=100",
            &make_streams_response(streams, None),
        );

        let client = TwitchClient::with_http_client("my_client_id".to_string(), mock.clone());
        client.set_access_token("my_access_token".to_string()).await;
        client.set_user_id("user123".to_string()).await;

        client.get_followed_streams().await.unwrap();

        let requests = mock.get_requests();
        assert_eq!(requests.len(), 1);

        let auth_header = requests[0].headers.get("Authorization").unwrap();
        assert_eq!(auth_header.to_str().unwrap(), "Bearer my_access_token");

        let client_id_header = requests[0].headers.get("Client-Id").unwrap();
        assert_eq!(client_id_header.to_str().unwrap(), "my_client_id");
    }

    // === search_categories tests ===

    #[tokio::test]
    async fn search_categories_returns_results() {
        let response = SearchCategoriesResponse {
            data: vec![
                Category {
                    id: "509658".to_string(),
                    name: "Just Chatting".to_string(),
                    box_art_url: "https://example.com/box.jpg".to_string(),
                },
                Category {
                    id: "27471".to_string(),
                    name: "Minecraft".to_string(),
                    box_art_url: "https://example.com/mc.jpg".to_string(),
                },
            ],
        };

        let mock = MockHttpClient::new().on_get_json(
            "https://api.twitch.tv/helix/search/categories?query=chat&first=10",
            &response,
        );

        let client = TwitchClient::with_http_client("test_client_id".to_string(), mock);
        client.set_access_token("test_token".to_string()).await;

        let result = client.search_categories("chat").await.unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, "509658");
        assert_eq!(result[0].name, "Just Chatting");
    }

    #[tokio::test]
    async fn search_categories_encodes_query() {
        let response = SearchCategoriesResponse { data: vec![] };

        let mock = MockHttpClient::new().on_get_json(
            "https://api.twitch.tv/helix/search/categories?query=just%20chatting&first=10",
            &response,
        );

        let client = TwitchClient::with_http_client("test_client_id".to_string(), mock);
        client.set_access_token("test_token".to_string()).await;

        let result = client.search_categories("just chatting").await.unwrap();
        assert!(result.is_empty());
    }

    // === get_streams_by_category tests ===

    #[tokio::test]
    async fn get_streams_by_category_returns_streams() {
        let streams = vec![
            make_stream("1", "StreamerOne"),
            make_stream("2", "StreamerTwo"),
        ];

        let mock = MockHttpClient::new().on_get_json(
            "https://api.twitch.tv/helix/streams?game_id=509658&first=10",
            &make_streams_response(streams, None),
        );

        let client = TwitchClient::with_http_client("test_client_id".to_string(), mock);
        client.set_access_token("test_token".to_string()).await;

        let result = client.get_streams_by_category("509658").await.unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].user_name, "StreamerOne");
        assert_eq!(result[1].user_name, "StreamerTwo");
    }

    #[tokio::test]
    async fn get_streams_by_category_empty() {
        let mock = MockHttpClient::new().on_get_json(
            "https://api.twitch.tv/helix/streams?game_id=999999&first=10",
            &make_streams_response(vec![], None),
        );

        let client = TwitchClient::with_http_client("test_client_id".to_string(), mock);
        client.set_access_token("test_token".to_string()).await;

        let result = client.get_streams_by_category("999999").await.unwrap();
        assert!(result.is_empty());
    }
}
