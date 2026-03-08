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
use crate::twitch::{ScheduleData, ScheduleVacation, ScheduledStream, TwitchClient};

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
    config: Arc<ConfigManager>,
    session: SessionManager,
}

impl ScheduleWalker {
    pub fn new(
        db: Database,
        client: TwitchClient,
        state: Arc<AppState>,
        config: Arc<ConfigManager>,
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

/// Returns true if the segment's time range overlaps with the vacation period.
///
/// A segment with no `end_time` is treated as a point event at `start_time`.
fn overlaps_vacation(seg: &crate::twitch::ScheduleSegment, vacation: &ScheduleVacation) -> bool {
    let seg_end = seg.end_time.unwrap_or(seg.start_time);
    seg.start_time < vacation.end_time && seg_end > vacation.start_time
}

/// Converts raw API schedule segments into [`ScheduledStream`] structs.
/// Skips canceled segments and segments that overlap with the broadcaster's vacation.
/// Does NOT filter by time horizon (stores all future segments).
fn convert_schedule_segments(data: &ScheduleData) -> Vec<ScheduledStream> {
    let Some(segments) = &data.segments else {
        return Vec::new();
    };

    segments
        .iter()
        .filter(|seg| seg.canceled_until.is_none())
        .filter(|seg| {
            data.vacation
                .as_ref()
                .is_none_or(|v| !overlaps_vacation(seg, v))
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::twitch::{ScheduleCategory, ScheduleSegment};
    use chrono::{Duration, TimeZone, Utc};

    fn make_schedule_data(
        segments: Vec<ScheduleSegment>,
        vacation: Option<ScheduleVacation>,
    ) -> ScheduleData {
        ScheduleData {
            segments: Some(segments),
            broadcaster_id: "123".to_string(),
            broadcaster_name: "TestBroadcaster".to_string(),
            broadcaster_login: "testbroadcaster".to_string(),
            vacation,
        }
    }

    fn make_segment(id: &str, start: chrono::DateTime<Utc>, hours: i64) -> ScheduleSegment {
        ScheduleSegment {
            id: id.to_string(),
            start_time: start,
            end_time: Some(start + Duration::hours(hours)),
            title: format!("Stream {id}"),
            canceled_until: None,
            category: Some(ScheduleCategory {
                id: "game1".to_string(),
                name: "Test Game".to_string(),
            }),
            is_recurring: false,
        }
    }

    #[test]
    fn canceled_segments_filtered_out() {
        let start = Utc::now() + Duration::hours(1);
        let mut seg = make_segment("1", start, 2);
        seg.canceled_until = Some("2026-01-01T00:00:00Z".to_string());

        let data = make_schedule_data(vec![seg], None);
        let result = convert_schedule_segments(&data);
        assert!(result.is_empty());
    }

    #[test]
    fn segments_during_vacation_filtered_out() {
        let vacation_start = Utc.with_ymd_and_hms(2026, 3, 10, 0, 0, 0).unwrap();
        let vacation_end = Utc.with_ymd_and_hms(2026, 3, 17, 0, 0, 0).unwrap();
        let vacation = ScheduleVacation {
            start_time: vacation_start,
            end_time: vacation_end,
        };

        // Segment fully inside vacation
        let inside = make_segment("inside", vacation_start + Duration::hours(12), 2);
        // Segment before vacation
        let before = make_segment("before", vacation_start - Duration::hours(5), 2);
        // Segment after vacation
        let after = make_segment("after", vacation_end + Duration::hours(1), 2);

        let data = make_schedule_data(vec![inside, before, after], Some(vacation));
        let result = convert_schedule_segments(&data);

        let ids: Vec<&str> = result.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["before", "after"]);
    }

    #[test]
    fn segment_overlapping_vacation_start_filtered_out() {
        let vacation_start = Utc.with_ymd_and_hms(2026, 3, 10, 0, 0, 0).unwrap();
        let vacation_end = Utc.with_ymd_and_hms(2026, 3, 17, 0, 0, 0).unwrap();
        let vacation = ScheduleVacation {
            start_time: vacation_start,
            end_time: vacation_end,
        };

        // Starts before vacation, ends during vacation
        let overlapping = make_segment("overlap", vacation_start - Duration::hours(1), 3);

        let data = make_schedule_data(vec![overlapping], Some(vacation));
        let result = convert_schedule_segments(&data);
        assert!(result.is_empty());
    }

    #[test]
    fn segment_ending_exactly_at_vacation_start_not_filtered() {
        let vacation_start = Utc.with_ymd_and_hms(2026, 3, 10, 0, 0, 0).unwrap();
        let vacation_end = Utc.with_ymd_and_hms(2026, 3, 17, 0, 0, 0).unwrap();
        let vacation = ScheduleVacation {
            start_time: vacation_start,
            end_time: vacation_end,
        };

        // Ends exactly when vacation starts — no overlap
        let adjacent = make_segment("adjacent", vacation_start - Duration::hours(2), 2);

        let data = make_schedule_data(vec![adjacent], Some(vacation));
        let result = convert_schedule_segments(&data);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "adjacent");
    }

    #[test]
    fn no_vacation_keeps_all_segments() {
        let start = Utc::now() + Duration::hours(1);
        let seg1 = make_segment("1", start, 2);
        let seg2 = make_segment("2", start + Duration::hours(3), 2);

        let data = make_schedule_data(vec![seg1, seg2], None);
        let result = convert_schedule_segments(&data);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn segment_category_and_title_preserved() {
        let start = Utc::now() + Duration::hours(1);
        let seg = make_segment("1", start, 2);

        let data = make_schedule_data(vec![seg], None);
        let result = convert_schedule_segments(&data);

        assert_eq!(result[0].title, "Stream 1");
        assert_eq!(result[0].category.as_deref(), Some("Test Game"));
        assert_eq!(result[0].category_id.as_deref(), Some("game1"));
    }

    #[test]
    fn null_segments_returns_empty() {
        let data = ScheduleData {
            segments: None,
            broadcaster_id: "123".to_string(),
            broadcaster_name: "Test".to_string(),
            broadcaster_login: "test".to_string(),
            vacation: None,
        };
        let result = convert_schedule_segments(&data);
        assert!(result.is_empty());
    }
}
