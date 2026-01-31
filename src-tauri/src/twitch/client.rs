use anyhow::{Context, Result};
use reqwest::Client;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::types::*;

const HELIX_BASE_URL: &str = "https://api.twitch.tv/helix";

/// Twitch Helix API client
pub struct TwitchClient {
    http: Client,
    client_id: String,
    access_token: Arc<RwLock<Option<String>>>,
    user_id: Arc<RwLock<Option<String>>>,
}

impl TwitchClient {
    /// Creates a new Twitch API client
    pub fn new(client_id: String) -> Self {
        Self {
            http: Client::new(),
            client_id,
            access_token: Arc::new(RwLock::new(None)),
            user_id: Arc::new(RwLock::new(None)),
        }
    }

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

    /// Gets the authenticated user's ID
    pub async fn get_user_id(&self) -> Option<String> {
        self.user_id.read().await.clone()
    }

    /// Gets the client ID
    pub fn get_client_id(&self) -> &str {
        &self.client_id
    }

    /// Makes an authenticated GET request to the Helix API
    async fn get<T: serde::de::DeserializeOwned>(&self, endpoint: &str) -> Result<T> {
        let token = self
            .access_token
            .read()
            .await
            .clone()
            .context("No access token set")?;

        let url = format!("{}{}", HELIX_BASE_URL, endpoint);

        let response = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Client-Id", &self.client_id)
            .send()
            .await
            .context("Failed to send request")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("API error {}: {}", status, body);
        }

        response.json().await.context("Failed to parse response")
    }

    /// Clears authentication state
    pub async fn clear_auth(&self) {
        *self.access_token.write().await = None;
        *self.user_id.write().await = None;
    }
}

impl Clone for TwitchClient {
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
impl TwitchClient {
    /// Gets live streams from channels the user follows
    pub async fn get_followed_streams(&self) -> Result<Vec<Stream>> {
        let user_id = self.get_user_id().await.context("User ID not set")?;

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
impl TwitchClient {
    /// Gets channels the user follows (paginated)
    pub async fn get_followed_channels(
        &self,
        cursor: Option<&str>,
    ) -> Result<(Vec<FollowedChannel>, Option<String>)> {
        let user_id = self.get_user_id().await.context("User ID not set")?;

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
    pub async fn get_all_followed_channels(&self) -> Result<Vec<FollowedChannel>> {
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

// Schedule-related methods
impl TwitchClient {
    /// Gets scheduled streams for a broadcaster
    async fn get_schedule(&self, broadcaster_id: &str) -> Result<Option<ScheduleData>> {
        let endpoint = format!("/schedule?broadcaster_id={}&first=10", broadcaster_id);

        // Schedule endpoint returns 404 if no schedule exists
        let token = self
            .access_token
            .read()
            .await
            .clone()
            .context("No access token set")?;

        let url = format!("{}{}", HELIX_BASE_URL, endpoint);

        let response = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Client-Id", &self.client_id)
            .send()
            .await
            .context("Failed to send request")?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            tracing::warn!("Schedule API error {}: {}", status, body);
            return Ok(None);
        }

        let schedule_response: ScheduleResponse = response
            .json()
            .await
            .context("Failed to parse schedule response")?;

        Ok(Some(schedule_response.data))
    }

    /// Gets scheduled streams for multiple broadcasters (next 24 hours)
    pub async fn get_scheduled_streams(
        &self,
        broadcaster_ids: &[String],
    ) -> Result<Vec<ScheduledStream>> {
        use chrono::Utc;

        let now = Utc::now();
        let cutoff = now + chrono::Duration::hours(24);
        let mut all_scheduled = Vec::new();

        for broadcaster_id in broadcaster_ids {
            let Some(schedule) = self.get_schedule(broadcaster_id).await? else {
                continue;
            };

            let Some(segments) = schedule.segments else {
                continue;
            };

            for segment in segments {
                // Skip if already started or past our 24h window
                if segment.start_time < now || segment.start_time > cutoff {
                    continue;
                }

                // Skip canceled segments
                if segment.canceled_until.is_some() {
                    continue;
                }

                let scheduled = ScheduledStream {
                    id: segment.id,
                    broadcaster_id: schedule.broadcaster_id.clone(),
                    broadcaster_name: schedule.broadcaster_name.clone(),
                    broadcaster_login: schedule.broadcaster_login.clone(),
                    title: segment.title,
                    start_time: segment.start_time,
                    end_time: segment.end_time,
                    category: segment.category.as_ref().map(|c| c.name.clone()),
                    category_id: segment.category.map(|c| c.id),
                    is_recurring: segment.is_recurring,
                };

                all_scheduled.push(scheduled);
            }
        }

        // Sort by start time
        all_scheduled.sort_by(|a, b| a.start_time.cmp(&b.start_time));

        tracing::debug!(
            "Returning {} scheduled streams within next 24h",
            all_scheduled.len()
        );

        Ok(all_scheduled)
    }

    /// Gets scheduled streams for all followed channels
    pub async fn get_scheduled_streams_for_followed(&self) -> Result<Vec<ScheduledStream>> {
        // First get all followed channels
        let follows = self.get_all_followed_channels().await?;
        tracing::debug!(
            "Got {} followed channels for schedule lookup",
            follows.len()
        );

        // Extract broadcaster IDs (limit to avoid too many API calls)
        let max_broadcasters = 50;
        let broadcaster_ids: Vec<String> = follows
            .into_iter()
            .take(max_broadcasters)
            .map(|f| f.broadcaster_id)
            .collect();

        let scheduled = self.get_scheduled_streams(&broadcaster_ids).await?;
        tracing::debug!("Found {} scheduled streams", scheduled.len());

        Ok(scheduled)
    }
}
