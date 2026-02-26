//! Schedule queue walker: checks one broadcaster's schedule per tick.
//!
//! Instead of bulk-fetching all channels at once, the walker picks the
//! most-stale broadcaster every `schedule_check_interval_sec` seconds and
//! fetches one at a time. This ensures all followed channels eventually get
//! a fresh schedule, not just the first 50.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use tokio::task::JoinHandle;
use tokio::time::Duration;

use crate::config::ConfigManager;
use crate::db::Database;
use crate::session::SessionManager;
use crate::state::AppState;
use crate::twitch::{ScheduleData, ScheduledStream, TwitchClient};

/// Within this many seconds, an inferred schedule is considered a duplicate of an API schedule.
const SCHEDULE_DEDUP_WINDOW_SECS: i64 = 3600;

/// Owns the schedule-refresh queue walk.
///
/// One broadcaster is checked per tick; results are stored in SQLite and read
/// back via [`ScheduleWalker::refresh_schedules_from_db`].
pub struct ScheduleWalker {
    db: Database,
    client: TwitchClient,
    state: Arc<AppState>,
    config: ConfigManager,
    session: SessionManager,
}

impl ScheduleWalker {
    pub fn new(
        db: Database,
        client: TwitchClient,
        state: Arc<AppState>,
        config: ConfigManager,
        session: SessionManager,
    ) -> Self {
        Self {
            db,
            client,
            state,
            config,
            session,
        }
    }

    /// Runs one iteration of the schedule queue: fetches the most-stale
    /// broadcaster's schedule and stores the result in the DB.
    pub async fn tick(&self) -> anyhow::Result<()> {
        if !self.state.is_authenticated().await {
            return Ok(());
        }

        let stale_threshold = (self.config.get().schedule_stale_hours * 3600) as i64;
        let broadcaster = match self.db.get_next_stale_broadcaster(stale_threshold) {
            Ok(Some(b)) => b,
            Ok(None) => return Ok(()), // All are fresh
            Err(e) => {
                tracing::error!("Failed to query schedule queue: {}", e);
                return Err(e);
            }
        };

        let (bid, blogin, bname) = broadcaster;
        let bid_str = bid.to_string();
        tracing::debug!("Checking schedule for {} ({})", bname, bid);

        match crate::twitch::with_retry(
            || self.client.get_schedule(&bid_str),
            || self.session.try_refresh_token(),
        )
        .await
        {
            Ok(Some(data)) => {
                let segments = convert_schedule_segments(&data);
                if let Err(e) = self.db.replace_future_schedules(bid, &segments) {
                    tracing::error!("Failed to store schedules for {}: {}", blogin, e);
                }
                if let Err(e) = self.db.update_last_checked(bid) {
                    tracing::error!("Failed to update last_checked for {}: {}", blogin, e);
                }
                self.refresh_schedules_from_db().await;
            }
            Ok(None) => {
                // No schedule (404) — clear future entries for this broadcaster
                if let Err(e) = self.db.replace_future_schedules(bid, &[]) {
                    tracing::error!("Failed to clear schedules for {}: {}", blogin, e);
                }
                if let Err(e) = self.db.update_last_checked(bid) {
                    tracing::error!("Failed to update last_checked for {}: {}", blogin, e);
                }
                self.refresh_schedules_from_db().await;
            }
            Err(e) => {
                // Don't update last_checked — will retry next cycle
                tracing::warn!("Failed to fetch schedule for {}: {}", blogin, e);
            }
        }

        Ok(())
    }

    /// Spawns the schedule walker polling loop.
    ///
    /// The tick interval is read from config on each iteration so that
    /// config changes take effect without a restart.
    pub fn start(self: Arc<Self>) -> JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                let tick_duration =
                    Duration::from_secs(self.config.get().schedule_check_interval_sec);
                tokio::time::sleep(tick_duration).await;
                if let Err(e) = self.tick().await {
                    tracing::error!("Schedule walker error: {}", e);
                }
            }
        })
    }

    /// Reads upcoming schedules from DB, merges with inferred schedules, and updates state.
    ///
    /// Both API and inferred schedules use the same display window:
    /// `[now - schedule_before_now_min, now + schedule_lookahead_hours]`.
    /// Deduplication removes inferred entries that overlap with an API schedule
    /// for the same broadcaster within 60 minutes.
    pub async fn refresh_schedules_from_db(&self) {
        let cfg = self.config.get();
        let now = Utc::now();
        let start = now - chrono::Duration::minutes(cfg.schedule_before_now_min as i64);
        let end = now + chrono::Duration::hours(cfg.schedule_lookahead_hours as i64);

        let db_schedules = match self.db.get_upcoming_schedules(start, end) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Failed to read schedules from DB: {}", e);
                return;
            }
        };

        // Infer schedules from stream history using the same window
        let channels = self.state.get_followed_channels().await;
        let channel_lookup: HashMap<String, _> = channels
            .into_iter()
            .map(|c| (c.broadcaster_id.clone(), c))
            .collect();

        let mut combined = db_schedules;
        match self.db.infer_schedules(&channel_lookup, start, end) {
            Ok(inferred) => {
                if !inferred.is_empty() {
                    // Deduplicate: skip inferred schedules that overlap with an
                    // API schedule for the same broadcaster within 60 minutes
                    let deduped: Vec<_> = inferred
                        .into_iter()
                        .filter(|inf| {
                            !combined.iter().any(|api| {
                                api.broadcaster_id == inf.broadcaster_id
                                    && (api.start_time - inf.start_time).num_seconds().abs()
                                        <= SCHEDULE_DEDUP_WINDOW_SECS
                            })
                        })
                        .collect();
                    if !deduped.is_empty() {
                        tracing::debug!("Inferred {} schedule(s) from history", deduped.len());
                        combined.extend(deduped);
                        combined.sort_by_key(|s| s.start_time);
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to infer schedules: {}", e);
            }
        }

        self.state.set_scheduled_streams(combined).await;
    }
}

/// Converts raw API schedule segments into [`ScheduledStream`] structs.
/// Skips canceled segments. Does NOT filter by time horizon (stores all future segments).
fn convert_schedule_segments(data: &ScheduleData) -> Vec<ScheduledStream> {
    let Some(segments) = &data.segments else {
        return Vec::new();
    };

    segments
        .iter()
        .filter(|seg| seg.canceled_until.is_none())
        .map(|seg| ScheduledStream {
            id: seg.id.clone(),
            broadcaster_id: data.broadcaster_id.clone(),
            broadcaster_name: data.broadcaster_name.clone(),
            broadcaster_login: data.broadcaster_login.clone(),
            title: seg.title.clone(),
            start_time: seg.start_time,
            end_time: seg.end_time,
            category: seg.category.as_ref().map(|c| c.name.clone()),
            category_id: seg.category.as_ref().map(|c| c.id.clone()),
            is_recurring: seg.is_recurring,
            is_inferred: false,
        })
        .collect()
}
