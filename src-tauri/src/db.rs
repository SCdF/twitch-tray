use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, Utc};
use rusqlite::{Connection, OptionalExtension};

use crate::twitch::{FollowedChannel, ScheduledStream, Stream};

/// Database for recording stream history, followed channels, and schedules.
#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    /// Opens or creates the database at the given path.
    ///
    /// If `history.db` exists in the same directory and `data.db` does not,
    /// renames it first (one-time migration).
    pub fn new(db_path: PathBuf) -> anyhow::Result<Self> {
        // Migrate history.db → data.db if needed
        if let Some(parent) = db_path.parent() {
            let old_path = parent.join("history.db");
            if old_path.exists() && !db_path.exists() {
                tracing::info!("Migrating {:?} → {:?}", old_path, db_path);
                std::fs::rename(&old_path, &db_path)?;
            }
        }

        let conn = Connection::open(&db_path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS stream_history (
                user_id INTEGER NOT NULL,
                started_at INTEGER NOT NULL,
                UNIQUE(user_id, started_at)
            );
            CREATE INDEX IF NOT EXISTS idx_stream_history_user_id
                ON stream_history(user_id);

            CREATE TABLE IF NOT EXISTS followed (
                broadcaster_id INTEGER PRIMARY KEY,
                broadcaster_login TEXT NOT NULL,
                broadcaster_name TEXT NOT NULL,
                followed_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS schedule_last_checked (
                broadcaster_id INTEGER PRIMARY KEY,
                last_checked_at INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS scheduled_streams (
                id TEXT NOT NULL,
                broadcaster_id INTEGER NOT NULL,
                title TEXT NOT NULL DEFAULT '',
                start_time INTEGER NOT NULL,
                end_time INTEGER,
                category_name TEXT,
                category_id INTEGER,
                is_recurring INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (id, broadcaster_id)
            );
            CREATE INDEX IF NOT EXISTS idx_scheduled_streams_start
                ON scheduled_streams(start_time);
            CREATE INDEX IF NOT EXISTS idx_scheduled_streams_broadcaster
                ON scheduled_streams(broadcaster_id);",
        )?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Records observed live streams into the history database.
    /// Uses INSERT OR IGNORE so duplicates are silently skipped.
    pub fn record_streams(&self, streams: &[Stream]) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "INSERT OR IGNORE INTO stream_history (user_id, started_at) VALUES (?1, ?2)",
        )?;
        for stream in streams {
            let user_id: i64 = stream.user_id.parse()?;
            let started_at = stream.started_at.timestamp();
            stmt.execute(rusqlite::params![user_id, started_at])?;
        }
        Ok(())
    }

    /// Returns the earliest recorded stream time for each of the given users.
    pub fn get_earliest_streams(
        &self,
        user_ids: &[i64],
    ) -> anyhow::Result<HashMap<i64, DateTime<Utc>>> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT user_id, MIN(started_at) FROM stream_history WHERE user_id IN ({}) GROUP BY user_id",
            repeat_vars(user_ids.len())
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(user_ids.iter()), |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut result = HashMap::new();
        for row in rows {
            let (uid, ts) = row?;
            if let Some(dt) = DateTime::from_timestamp(ts, 0) {
                result.insert(uid, dt);
            }
        }
        Ok(result)
    }

    /// Returns stream start times for all given users within the given time range,
    /// grouped by user_id.
    pub fn get_streams_in_range(
        &self,
        user_ids: &[i64],
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> anyhow::Result<HashMap<i64, Vec<DateTime<Utc>>>> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT user_id, started_at FROM stream_history WHERE user_id IN ({}) AND started_at >= ? AND started_at <= ?",
            repeat_vars(user_ids.len())
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut params: Vec<i64> = user_ids.to_vec();
        params.push(from.timestamp());
        params.push(to.timestamp());
        let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut result: HashMap<i64, Vec<DateTime<Utc>>> = HashMap::new();
        for row in rows {
            let (uid, ts) = row?;
            if let Some(dt) = DateTime::from_timestamp(ts, 0) {
                result.entry(uid).or_default().push(dt);
            }
        }
        Ok(result)
    }

    // === Followed channels ===

    /// Replaces the `followed` table with the current list of followed channels.
    pub fn sync_followed(&self, channels: &[FollowedChannel]) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let tx = conn.unchecked_transaction()?;
        tx.execute("DELETE FROM followed", [])?;
        let mut stmt = tx.prepare(
            "INSERT INTO followed (broadcaster_id, broadcaster_login, broadcaster_name, followed_at)
             VALUES (?1, ?2, ?3, ?4)",
        )?;
        for ch in channels {
            let id: i64 = ch.broadcaster_id.parse()?;
            stmt.execute(rusqlite::params![
                id,
                ch.broadcaster_login,
                ch.broadcaster_name,
                ch.followed_at.timestamp()
            ])?;
        }
        drop(stmt);
        tx.commit()?;
        Ok(())
    }

    /// Returns all broadcaster IDs from the `followed` table.
    pub fn get_followed_ids(&self) -> anyhow::Result<Vec<i64>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT broadcaster_id FROM followed")?;
        let rows = stmt.query_map([], |row| row.get::<_, i64>(0))?;
        let mut ids = Vec::new();
        for row in rows {
            ids.push(row?);
        }
        Ok(ids)
    }

    // === Schedule queue ===

    /// Ensures every broadcaster in the list has an entry in `schedule_last_checked`.
    /// New entries get `last_checked_at = 0` (immediately stale). Existing entries are untouched.
    pub fn ensure_schedule_queue_entries(&self, broadcaster_ids: &[i64]) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "INSERT OR IGNORE INTO schedule_last_checked (broadcaster_id, last_checked_at) VALUES (?1, 0)",
        )?;
        for &id in broadcaster_ids {
            stmt.execute(rusqlite::params![id])?;
        }
        Ok(())
    }

    /// Returns the most-stale currently-followed broadcaster whose schedule
    /// hasn't been checked within `stale_threshold_secs` seconds.
    pub fn get_next_stale_broadcaster(
        &self,
        stale_threshold_secs: i64,
    ) -> anyhow::Result<Option<(i64, String, String)>> {
        let conn = self.conn.lock().unwrap();
        let threshold = Utc::now().timestamp() - stale_threshold_secs;
        let mut stmt = conn.prepare(
            "SELECT f.broadcaster_id, f.broadcaster_login, f.broadcaster_name
             FROM followed f
             JOIN schedule_last_checked s ON f.broadcaster_id = s.broadcaster_id
             WHERE s.last_checked_at < ?1
             ORDER BY s.last_checked_at ASC
             LIMIT 1",
        )?;
        let result = stmt
            .query_row(rusqlite::params![threshold], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .optional()?;
        Ok(result)
    }

    /// Marks a broadcaster's schedule as just-checked.
    pub fn update_last_checked(&self, broadcaster_id: i64) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();
        conn.execute(
            "UPDATE schedule_last_checked SET last_checked_at = ?1 WHERE broadcaster_id = ?2",
            rusqlite::params![now, broadcaster_id],
        )?;
        Ok(())
    }

    // === Scheduled streams ===

    /// Replaces future scheduled streams for a broadcaster.
    /// Past streams (start_time < now) are preserved.
    pub fn replace_future_schedules(
        &self,
        broadcaster_id: i64,
        streams: &[ScheduledStream],
    ) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM scheduled_streams WHERE broadcaster_id = ?1 AND start_time >= ?2",
            rusqlite::params![broadcaster_id, now],
        )?;
        let mut stmt = tx.prepare(
            "INSERT OR REPLACE INTO scheduled_streams
             (id, broadcaster_id, title, start_time, end_time, category_name, category_id, is_recurring)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?;
        for s in streams {
            let cat_id: Option<i64> = s.category_id.as_ref().and_then(|id| id.parse().ok());
            stmt.execute(rusqlite::params![
                s.id,
                broadcaster_id,
                s.title,
                s.start_time.timestamp(),
                s.end_time.map(|t| t.timestamp()),
                s.category,
                cat_id,
                s.is_recurring as i64,
            ])?;
        }
        drop(stmt);
        tx.commit()?;
        Ok(())
    }

    /// Returns scheduled streams in the `[start, end]` window,
    /// filtered to only currently-followed broadcasters via JOIN.
    pub fn get_upcoming_schedules(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> anyhow::Result<Vec<ScheduledStream>> {
        let conn = self.conn.lock().unwrap();
        let start = start.timestamp();
        let end = end.timestamp();
        let mut stmt = conn.prepare(
            "SELECT ss.id, ss.broadcaster_id, f.broadcaster_login, f.broadcaster_name,
                    ss.title, ss.start_time, ss.end_time, ss.category_name, ss.category_id,
                    ss.is_recurring
             FROM scheduled_streams ss
             JOIN followed f ON ss.broadcaster_id = f.broadcaster_id
             WHERE ss.start_time BETWEEN ?1 AND ?2
             ORDER BY ss.start_time",
        )?;
        let rows = stmt.query_map(rusqlite::params![start, end], |row| {
            Ok((
                row.get::<_, String>(0)?,         // id
                row.get::<_, i64>(1)?,            // broadcaster_id
                row.get::<_, String>(2)?,         // broadcaster_login
                row.get::<_, String>(3)?,         // broadcaster_name
                row.get::<_, String>(4)?,         // title
                row.get::<_, i64>(5)?,            // start_time
                row.get::<_, Option<i64>>(6)?,    // end_time
                row.get::<_, Option<String>>(7)?, // category_name
                row.get::<_, Option<i64>>(8)?,    // category_id
                row.get::<_, i64>(9)?,            // is_recurring
            ))
        })?;
        let mut schedules = Vec::new();
        for row in rows {
            let (id, bid, login, name, title, start, end, cat, cat_id, recurring) = row?;
            schedules.push(ScheduledStream {
                id,
                broadcaster_id: bid.to_string(),
                broadcaster_login: login,
                broadcaster_name: name,
                title,
                start_time: DateTime::from_timestamp(start, 0).unwrap_or_else(Utc::now),
                end_time: end.and_then(|t| DateTime::from_timestamp(t, 0)),
                category: cat,
                category_id: cat_id.map(|c| c.to_string()),
                is_recurring: recurring != 0,
                is_inferred: false,
            });
        }
        Ok(schedules)
    }

    /// Infers future schedules from historical stream data.
    ///
    /// Uses a weekly-recurrence heuristic: looks at the same time window
    /// shifted back 1, 2, and 3 weeks. If a streamer consistently goes live
    /// at similar times across multiple weeks, predicts they'll do so again.
    ///
    /// `start` and `end` define the prediction window (same clamp as API schedules).
    ///
    /// Issues exactly 4 SQL queries regardless of channel count:
    /// 1 for earliest streams, 3 for each lookback window.
    pub fn infer_schedules(
        &self,
        channel_lookup: &HashMap<String, FollowedChannel>,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> anyhow::Result<Vec<ScheduledStream>> {
        let window_start = start;
        let window_end = end;
        let window_secs = (window_end - window_start).num_seconds();

        // Collect all user IDs upfront
        let user_id_map: HashMap<i64, &str> = channel_lookup
            .iter()
            .filter_map(|(id_str, _)| id_str.parse::<i64>().ok().map(|id| (id, id_str.as_str())))
            .collect();
        let all_user_ids: Vec<i64> = user_id_map.keys().copied().collect();

        if all_user_ids.is_empty() {
            return Ok(Vec::new());
        }

        // Query 1: earliest stream per user (single query for all users)
        let earliest_map = self.get_earliest_streams(&all_user_ids)?;

        // Define lookback windows (same 25h window shifted back by 1/2/3 weeks)
        let lookback_windows: Vec<(DateTime<Utc>, DateTime<Utc>)> = (1..=3)
            .map(|weeks| {
                let shift = Duration::weeks(weeks);
                (window_start - shift, window_end - shift)
            })
            .collect();

        // Queries 2-4: streams in each lookback window (one query per window)
        let mut window_streams: Vec<HashMap<i64, Vec<DateTime<Utc>>>> =
            Vec::with_capacity(lookback_windows.len());
        for (win_start, win_end) in &lookback_windows {
            window_streams.push(self.get_streams_in_range(&all_user_ids, *win_start, *win_end)?);
        }

        // Now process each channel using only in-memory data
        let mut inferred = Vec::new();

        for (user_id, id_str) in &user_id_map {
            let channel = &channel_lookup[*id_str];

            let earliest = match earliest_map.get(user_id) {
                Some(e) => *e,
                None => continue, // No data for this streamer
            };

            // Determine which lookback windows have data coverage
            let mut weeks_with_data = 0;
            let mut valid_windows = Vec::new();
            for (i, (win_start, _win_end)) in lookback_windows.iter().enumerate() {
                if *win_start >= earliest {
                    weeks_with_data += 1;
                    valid_windows.push(i);
                }
            }

            if weeks_with_data == 0 {
                continue;
            }

            // Threshold: max(1, weeks_with_data - 1)
            let threshold = std::cmp::max(1, weeks_with_data - 1);

            // Collect (offset_seconds, week_index) pairs from valid windows
            let mut offset_week_pairs: Vec<(i64, usize)> = Vec::new();

            for &win_idx in &valid_windows {
                let week_number = win_idx + 1; // 1-indexed week number

                if let Some(streams) = window_streams[win_idx].get(user_id) {
                    for stream_time in streams {
                        let offset = (*stream_time
                            - (window_start - Duration::weeks(week_number as i64)))
                        .num_seconds();
                        // Clamp to window
                        if offset >= 0 && offset <= window_secs {
                            offset_week_pairs.push((offset, week_number));
                        }
                    }
                }
            }

            if offset_week_pairs.is_empty() {
                continue;
            }

            // Cluster using single-linkage with 3600s threshold
            let clusters = cluster_offsets(&offset_week_pairs, 3600);

            for cluster in clusters {
                // Count distinct weeks in this cluster
                let mut distinct_weeks: Vec<usize> = cluster.iter().map(|&(_, w)| w).collect();
                distinct_weeks.sort_unstable();
                distinct_weeks.dedup();

                if distinct_weeks.len() < threshold {
                    continue;
                }

                // Compute average offset
                let sum: i64 = cluster.iter().map(|&(o, _)| o).sum();
                let avg = sum as f64 / cluster.len() as f64;

                // Round to nearest 15 minutes (900s)
                let rounded = ((avg / 900.0).round() as i64) * 900;

                // Convert to absolute time
                let predicted_time = window_start + Duration::seconds(rounded);

                // Skip if outside the display window
                if predicted_time < start {
                    continue;
                }

                inferred.push(ScheduledStream {
                    id: format!("inferred_{}_{}", user_id, rounded),
                    broadcaster_id: channel.broadcaster_id.clone(),
                    broadcaster_name: channel.broadcaster_name.clone(),
                    broadcaster_login: channel.broadcaster_login.clone(),
                    title: String::new(),
                    start_time: predicted_time,
                    end_time: None,
                    category: None,
                    category_id: None,
                    is_recurring: false,
                    is_inferred: true,
                });
            }
        }

        // Sort by start time
        inferred.sort_by_key(|s| s.start_time);
        Ok(inferred)
    }
}

/// Generates `count` SQL placeholders: "?,?,?"
fn repeat_vars(count: usize) -> String {
    let mut s = "?,".repeat(count);
    s.pop(); // remove trailing comma
    s
}

/// Clusters (offset, week) pairs using single-linkage with the given threshold.
///
/// Sorts by offset, then greedily builds clusters: a new item joins the current
/// cluster if it's within `threshold_secs` of any existing cluster member.
fn cluster_offsets(pairs: &[(i64, usize)], threshold_secs: i64) -> Vec<Vec<(i64, usize)>> {
    if pairs.is_empty() {
        return Vec::new();
    }

    let mut sorted = pairs.to_vec();
    sorted.sort_by_key(|&(offset, _)| offset);

    let mut clusters: Vec<Vec<(i64, usize)>> = Vec::new();
    let mut used = vec![false; sorted.len()];

    for i in 0..sorted.len() {
        if used[i] {
            continue;
        }

        let mut cluster = vec![sorted[i]];
        used[i] = true;

        // Expand cluster: keep checking if any remaining item is within threshold
        // of any item already in the cluster
        let mut changed = true;
        while changed {
            changed = false;
            for j in 0..sorted.len() {
                if used[j] {
                    continue;
                }
                let close_to_any = cluster
                    .iter()
                    .any(|&(co, _)| (sorted[j].0 - co).abs() <= threshold_secs);
                if close_to_any {
                    cluster.push(sorted[j]);
                    used[j] = true;
                    changed = true;
                }
            }
        }

        clusters.push(cluster);
    }

    clusters
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn make_test_stream(user_id: &str, started_at: DateTime<Utc>) -> Stream {
        Stream {
            id: format!("stream_{}", user_id),
            user_id: user_id.to_string(),
            user_login: format!("user_{}", user_id),
            user_name: format!("User {}", user_id),
            game_id: "game123".to_string(),
            game_name: "Test Game".to_string(),
            title: "Test Stream".to_string(),
            viewer_count: 1000,
            started_at,
            thumbnail_url: "https://example.com/thumb.jpg".to_string(),
            tags: vec![],
        }
    }

    fn make_channel(id: &str, name: &str) -> FollowedChannel {
        FollowedChannel {
            broadcaster_id: id.to_string(),
            broadcaster_login: name.to_lowercase(),
            broadcaster_name: name.to_string(),
            followed_at: Utc::now(),
        }
    }

    fn in_memory_db() -> Database {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS stream_history (
                user_id INTEGER NOT NULL,
                started_at INTEGER NOT NULL,
                UNIQUE(user_id, started_at)
            );
            CREATE INDEX IF NOT EXISTS idx_stream_history_user_id
                ON stream_history(user_id);

            CREATE TABLE IF NOT EXISTS followed (
                broadcaster_id INTEGER PRIMARY KEY,
                broadcaster_login TEXT NOT NULL,
                broadcaster_name TEXT NOT NULL,
                followed_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS schedule_last_checked (
                broadcaster_id INTEGER PRIMARY KEY,
                last_checked_at INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS scheduled_streams (
                id TEXT NOT NULL,
                broadcaster_id INTEGER NOT NULL,
                title TEXT NOT NULL DEFAULT '',
                start_time INTEGER NOT NULL,
                end_time INTEGER,
                category_name TEXT,
                category_id INTEGER,
                is_recurring INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (id, broadcaster_id)
            );
            CREATE INDEX IF NOT EXISTS idx_scheduled_streams_start
                ON scheduled_streams(start_time);
            CREATE INDEX IF NOT EXISTS idx_scheduled_streams_broadcaster
                ON scheduled_streams(broadcaster_id);",
        )
        .unwrap();
        Database {
            conn: Arc::new(Mutex::new(conn)),
        }
    }

    // === DB / constraint tests ===

    #[test]
    fn table_creation_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test_history.db");
        let _db = Database::new(db_path).unwrap();
    }

    #[test]
    fn duplicate_insert_does_not_error() {
        let db = in_memory_db();
        let now = Utc::now();
        let stream = make_test_stream("141981764", now);
        db.record_streams(&[stream.clone()]).unwrap();
        db.record_streams(&[stream]).unwrap(); // same (user_id, started_at)
    }

    #[test]
    fn index_exists_on_user_id() {
        let db = in_memory_db();
        let conn = db.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='index' AND name='idx_stream_history_user_id'")
            .unwrap();
        let exists: bool = stmt.exists([]).unwrap();
        assert!(exists, "Index idx_stream_history_user_id should exist");
    }

    #[test]
    fn query_returns_correct_date_range() {
        let db = in_memory_db();
        let base = Utc.with_ymd_and_hms(2025, 6, 15, 12, 0, 0).unwrap();

        // Insert streams at different times
        let s1 = make_test_stream("100", base);
        let s2 = make_test_stream("100", base + Duration::hours(2));
        let s3 = make_test_stream("100", base + Duration::hours(5));
        db.record_streams(&[s1, s2, s3]).unwrap();

        // Query a range that includes only the first two
        let results = db
            .get_streams_in_range(&[100], base, base + Duration::hours(3))
            .unwrap();
        assert_eq!(results.get(&100).unwrap().len(), 2);
    }

    #[test]
    fn user_id_stored_as_integer() {
        let db = in_memory_db();
        let now = Utc::now();
        let stream = make_test_stream("141981764", now);
        db.record_streams(&[stream]).unwrap();

        let conn = db.conn.lock().unwrap();
        let stored: i64 = conn
            .query_row("SELECT user_id FROM stream_history LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(stored, 141981764);
    }

    #[test]
    fn started_at_stored_as_integer() {
        let db = in_memory_db();
        let now = Utc.with_ymd_and_hms(2025, 6, 15, 12, 0, 0).unwrap();
        let stream = make_test_stream("100", now);
        db.record_streams(&[stream]).unwrap();

        let conn = db.conn.lock().unwrap();
        let stored: i64 = conn
            .query_row("SELECT started_at FROM stream_history LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(stored, now.timestamp());
    }

    // === Inference logic tests ===

    /// Helper to compute the schedule window from a given `now` time,
    /// matching the defaults used in production (15min before, 6h ahead).
    fn schedule_window(now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
        (now - Duration::minutes(15), now + Duration::hours(6))
    }

    #[test]
    fn three_weeks_two_match_predicted() {
        let db = in_memory_db();
        // "now" is a fixed point: Wednesday at 14:00 UTC
        let now = Utc.with_ymd_and_hms(2025, 7, 16, 14, 0, 0).unwrap();
        let (start, end) = schedule_window(now);

        // Need early record to establish coverage for all 3 weeks
        // Week 3 window starts at start-21d = June 25 13:45
        let earliest = Utc.with_ymd_and_hms(2025, 6, 25, 12, 0, 0).unwrap();

        // Streamer went live at ~15:00 on 2 of the last 3 weeks (Wed)
        let w1 = Utc.with_ymd_and_hms(2025, 7, 9, 15, 0, 0).unwrap();
        let w2 = Utc.with_ymd_and_hms(2025, 7, 2, 15, 0, 0).unwrap();
        // Week 3: skip (only 2/3 match)

        db.record_streams(&[
            make_test_stream("100", earliest),
            make_test_stream("100", w1),
            make_test_stream("100", w2),
        ])
        .unwrap();

        let mut channels = HashMap::new();
        channels.insert("100".to_string(), make_channel("100", "TestStreamer"));

        let result = db.infer_schedules(&channels, start, end).unwrap();
        assert_eq!(result.len(), 1, "Should predict one schedule");
        assert!(result[0].is_inferred);
        // Predicted time should be around 15:00
        assert_eq!(result[0].start_time.hour(), 15);
    }

    #[test]
    fn three_weeks_one_match_not_predicted() {
        let db = in_memory_db();
        let now = Utc.with_ymd_and_hms(2025, 7, 16, 14, 0, 0).unwrap();
        let (start, end) = schedule_window(now);

        // Only 1 out of 3 weeks has a stream at this time
        // We need 3 weeks of data but only 1 match → threshold is 2, so not predicted
        let w1 = Utc.with_ymd_and_hms(2025, 7, 9, 15, 0, 0).unwrap();
        // Add a stream far in the past so weeks 2 and 3 count as "having data"
        let w2_early = Utc.with_ymd_and_hms(2025, 7, 2, 3, 0, 0).unwrap(); // 3am, outside window
        let w3_early = Utc.with_ymd_and_hms(2025, 6, 25, 3, 0, 0).unwrap(); // 3am, outside window

        db.record_streams(&[
            make_test_stream("100", w1),
            make_test_stream("100", w2_early),
            make_test_stream("100", w3_early),
        ])
        .unwrap();

        let mut channels = HashMap::new();
        channels.insert("100".to_string(), make_channel("100", "TestStreamer"));

        let result = db.infer_schedules(&channels, start, end).unwrap();
        assert!(
            result.is_empty(),
            "Should not predict with only 1/3 weeks matching"
        );
    }

    #[test]
    fn two_weeks_both_match_predicted() {
        let db = in_memory_db();
        let now = Utc.with_ymd_and_hms(2025, 7, 16, 14, 0, 0).unwrap();
        let (start, end) = schedule_window(now);

        // Need early record to cover week 2 window (starts start-14d)
        let earliest = Utc.with_ymd_and_hms(2025, 7, 2, 12, 0, 0).unwrap();

        let w1 = Utc.with_ymd_and_hms(2025, 7, 9, 15, 0, 0).unwrap();
        let w2 = Utc.with_ymd_and_hms(2025, 7, 2, 15, 0, 0).unwrap();

        db.record_streams(&[
            make_test_stream("100", earliest),
            make_test_stream("100", w1),
            make_test_stream("100", w2),
        ])
        .unwrap();

        let mut channels = HashMap::new();
        channels.insert("100".to_string(), make_channel("100", "TestStreamer"));

        let result = db.infer_schedules(&channels, start, end).unwrap();
        assert_eq!(result.len(), 1, "Should predict with 2/2 weeks matching");
    }

    #[test]
    fn two_weeks_one_stream_predicted() {
        let db = in_memory_db();
        let now = Utc.with_ymd_and_hms(2025, 7, 16, 14, 0, 0).unwrap();
        let (start, end) = schedule_window(now);

        // Data goes back 2 weeks, but only week 1 has a stream in window
        // threshold = max(1, 2-1) = 1, so 1 match is enough
        let w1 = Utc.with_ymd_and_hms(2025, 7, 9, 15, 0, 0).unwrap();
        // w2 has a stream but outside the window (to establish data coverage)
        let w2_outside = Utc.with_ymd_and_hms(2025, 7, 2, 3, 0, 0).unwrap();

        db.record_streams(&[
            make_test_stream("100", w1),
            make_test_stream("100", w2_outside),
        ])
        .unwrap();

        let mut channels = HashMap::new();
        channels.insert("100".to_string(), make_channel("100", "TestStreamer"));

        let result = db.infer_schedules(&channels, start, end).unwrap();
        assert_eq!(
            result.len(),
            1,
            "Should predict with 1 match when threshold is 1"
        );
    }

    #[test]
    fn one_week_data_predicted() {
        let db = in_memory_db();
        let now = Utc.with_ymd_and_hms(2025, 7, 16, 14, 0, 0).unwrap();
        let (start, end) = schedule_window(now);

        // Week 1 lookback starts at start-7d = July 9 13:45
        // Need earliest record at or before that to have coverage
        let early = Utc.with_ymd_and_hms(2025, 7, 9, 13, 0, 0).unwrap();
        let w1 = Utc.with_ymd_and_hms(2025, 7, 9, 15, 0, 0).unwrap();

        db.record_streams(&[make_test_stream("100", early), make_test_stream("100", w1)])
            .unwrap();

        let mut channels = HashMap::new();
        channels.insert("100".to_string(), make_channel("100", "TestStreamer"));

        let result = db.infer_schedules(&channels, start, end).unwrap();
        assert_eq!(result.len(), 1, "Should predict with 1 week of data");
    }

    #[test]
    fn no_data_no_prediction() {
        let db = in_memory_db();
        let now = Utc.with_ymd_and_hms(2025, 7, 16, 14, 0, 0).unwrap();
        let (start, end) = schedule_window(now);

        let mut channels = HashMap::new();
        channels.insert("100".to_string(), make_channel("100", "TestStreamer"));

        let result = db.infer_schedules(&channels, start, end).unwrap();
        assert!(result.is_empty(), "No data should produce no predictions");
    }

    #[test]
    fn clustering_59min_apart_same_cluster() {
        let db = in_memory_db();
        let now = Utc.with_ymd_and_hms(2025, 7, 16, 14, 0, 0).unwrap();
        let (start, end) = schedule_window(now);

        // Need early record to cover week 2 window
        let earliest = Utc.with_ymd_and_hms(2025, 7, 2, 12, 0, 0).unwrap();

        // Two streams 1 minute apart should cluster together (within 60min threshold)
        // w1 at 15:55, w2 at 15:54 → offsets differ by 60s
        let w1 = Utc.with_ymd_and_hms(2025, 7, 9, 15, 55, 0).unwrap();
        let w2 = Utc.with_ymd_and_hms(2025, 7, 2, 15, 54, 0).unwrap();

        db.record_streams(&[
            make_test_stream("100", earliest),
            make_test_stream("100", w1),
            make_test_stream("100", w2),
        ])
        .unwrap();

        let mut channels = HashMap::new();
        channels.insert("100".to_string(), make_channel("100", "TestStreamer"));

        let result = db.infer_schedules(&channels, start, end).unwrap();
        assert_eq!(
            result.len(),
            1,
            "Streams 1min apart should cluster into one prediction"
        );
    }

    #[test]
    fn clustering_61min_apart_different_clusters() {
        let db = in_memory_db();
        let now = Utc.with_ymd_and_hms(2025, 7, 16, 14, 0, 0).unwrap();
        let (start, end) = schedule_window(now);

        // Need early record to establish coverage for both weeks
        let earliest = Utc.with_ymd_and_hms(2025, 7, 2, 12, 0, 0).unwrap();

        // Two streams 61 minutes apart in different weeks should NOT cluster
        // Week 1 at 15:00, week 2 at 16:01
        let w1 = Utc.with_ymd_and_hms(2025, 7, 9, 15, 0, 0).unwrap();
        let w2 = Utc.with_ymd_and_hms(2025, 7, 2, 16, 1, 0).unwrap();

        db.record_streams(&[
            make_test_stream("100", earliest),
            make_test_stream("100", w1),
            make_test_stream("100", w2),
        ])
        .unwrap();

        let mut channels = HashMap::new();
        channels.insert("100".to_string(), make_channel("100", "TestStreamer"));

        let result = db.infer_schedules(&channels, start, end).unwrap();
        // Each cluster has only 1 distinct week, threshold for 2 weeks data is 1
        // So both should be predicted separately
        assert_eq!(
            result.len(),
            2,
            "61min apart should be two separate predictions"
        );
    }

    #[test]
    fn average_and_rounding() {
        let db = in_memory_db();
        let now = Utc.with_ymd_and_hms(2025, 7, 16, 10, 0, 0).unwrap();
        let (start, end) = schedule_window(now);

        // Need an early record to establish coverage for all 3 weeks
        let earliest = Utc.with_ymd_and_hms(2025, 6, 25, 8, 0, 0).unwrap();

        // Three streams at 12:45, 12:58, 13:15 (within 30min, so they cluster)
        // Offsets from window_start (start = 9:45): 3h00m, 3h13m, 3h30m
        // = 10800, 11580, 12600 seconds
        // avg = 11660s → /900 = 12.955 → round to 13 → 13*900=11700s = 3h15m from 9:45 = 13:00
        let w1 = Utc.with_ymd_and_hms(2025, 7, 9, 12, 45, 0).unwrap();
        let w2 = Utc.with_ymd_and_hms(2025, 7, 2, 12, 58, 0).unwrap();
        let w3 = Utc.with_ymd_and_hms(2025, 6, 25, 13, 15, 0).unwrap();

        db.record_streams(&[
            make_test_stream("100", earliest),
            make_test_stream("100", w1),
            make_test_stream("100", w2),
            make_test_stream("100", w3),
        ])
        .unwrap();

        let mut channels = HashMap::new();
        channels.insert("100".to_string(), make_channel("100", "TestStreamer"));

        let result = db.infer_schedules(&channels, start, end).unwrap();
        assert_eq!(result.len(), 1);
        // Should round to nearest 15 min → 13:00
        assert_eq!(result[0].start_time.hour(), 13);
        assert_eq!(result[0].start_time.minute(), 0);
    }

    #[test]
    fn multiple_streamers_independent() {
        let db = in_memory_db();
        let now = Utc.with_ymd_and_hms(2025, 7, 16, 14, 0, 0).unwrap();
        let (start, end) = schedule_window(now);

        // Early records to establish coverage for both streamers
        let early100 = make_test_stream("100", Utc.with_ymd_and_hms(2025, 7, 2, 12, 0, 0).unwrap());
        let early200 = make_test_stream("200", Utc.with_ymd_and_hms(2025, 7, 2, 12, 0, 0).unwrap());

        // Streamer 100 streams at 15:00, streamer 200 at 18:00
        let s100_w1 = make_test_stream("100", Utc.with_ymd_and_hms(2025, 7, 9, 15, 0, 0).unwrap());
        let s100_w2 = make_test_stream("100", Utc.with_ymd_and_hms(2025, 7, 2, 15, 0, 0).unwrap());
        let s200_w1 = make_test_stream("200", Utc.with_ymd_and_hms(2025, 7, 9, 18, 0, 0).unwrap());
        let s200_w2 = make_test_stream("200", Utc.with_ymd_and_hms(2025, 7, 2, 18, 0, 0).unwrap());

        db.record_streams(&[early100, early200, s100_w1, s100_w2, s200_w1, s200_w2])
            .unwrap();

        let mut channels = HashMap::new();
        channels.insert("100".to_string(), make_channel("100", "StreamerA"));
        channels.insert("200".to_string(), make_channel("200", "StreamerB"));

        let result = db.infer_schedules(&channels, start, end).unwrap();
        assert_eq!(result.len(), 2, "Should predict for both streamers");

        let names: Vec<&str> = result.iter().map(|s| s.broadcaster_name.as_str()).collect();
        assert!(names.contains(&"StreamerA"));
        assert!(names.contains(&"StreamerB"));
    }

    #[test]
    fn weeks_with_data_excludes_before_earliest() {
        let db = in_memory_db();
        let now = Utc.with_ymd_and_hms(2025, 7, 16, 14, 0, 0).unwrap();
        let (start, end) = schedule_window(now);

        // Week 1 lookback starts at start-7d = July 9 13:45
        // Week 2 lookback starts at start-14d = July 2 13:45
        // Earliest record is July 6 (10 days ago) → week 1 valid, week 2 not
        let earliest = Utc.with_ymd_and_hms(2025, 7, 6, 12, 0, 0).unwrap();
        let w1 = Utc.with_ymd_and_hms(2025, 7, 9, 15, 0, 0).unwrap();

        db.record_streams(&[
            make_test_stream("100", earliest),
            make_test_stream("100", w1),
        ])
        .unwrap();

        let mut channels = HashMap::new();
        channels.insert("100".to_string(), make_channel("100", "TestStreamer"));

        let result = db.infer_schedules(&channels, start, end).unwrap();
        // Only 1 week of data → threshold = max(1, 1-1) = 1
        // 1 match >= 1 threshold → predicted
        assert_eq!(result.len(), 1);
    }

    // === cluster_offsets unit tests ===

    #[test]
    fn cluster_empty() {
        let result = cluster_offsets(&[], 3600);
        assert!(result.is_empty());
    }

    #[test]
    fn cluster_single_item() {
        let pairs = vec![(1000, 1)];
        let result = cluster_offsets(&pairs, 3600);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].len(), 1);
    }

    #[test]
    fn cluster_two_close() {
        let pairs = vec![(1000, 1), (2000, 2)];
        let result = cluster_offsets(&pairs, 3600);
        assert_eq!(
            result.len(),
            1,
            "Items 1000s apart should cluster with 3600s threshold"
        );
        assert_eq!(result[0].len(), 2);
    }

    #[test]
    fn cluster_two_far() {
        // 4000s apart > 3600s threshold → separate clusters
        let pairs = vec![(1000, 1), (5000, 2)];
        let result = cluster_offsets(&pairs, 3600);
        assert_eq!(
            result.len(),
            2,
            "Items 4000s apart should not cluster with 3600s threshold"
        );

        let pairs = vec![(1000, 1), (8000, 2)];
        let result = cluster_offsets(&pairs, 3600);
        assert_eq!(result.len(), 2, "Items 7000s apart should not cluster");
    }

    #[test]
    fn cluster_chain() {
        // A-B are 2000 apart, B-C are 2000 apart → A-C are 4000 apart
        // With single-linkage, all should be in one cluster since each adjacent pair is within threshold
        let pairs = vec![(1000, 1), (3000, 2), (5000, 3)];
        let result = cluster_offsets(&pairs, 3600);
        assert_eq!(
            result.len(),
            1,
            "Single-linkage should chain A-B-C into one cluster"
        );
        assert_eq!(result[0].len(), 3);
    }

    // === sync_followed tests ===

    #[test]
    fn sync_followed_inserts_channels() {
        let db = in_memory_db();
        let channels = vec![
            make_channel("100", "StreamerA"),
            make_channel("200", "StreamerB"),
        ];
        db.sync_followed(&channels).unwrap();

        let ids = db.get_followed_ids().unwrap();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&100));
        assert!(ids.contains(&200));
    }

    #[test]
    fn sync_followed_replaces_previous() {
        let db = in_memory_db();

        // First sync
        db.sync_followed(&[make_channel("100", "StreamerA")])
            .unwrap();
        assert_eq!(db.get_followed_ids().unwrap().len(), 1);

        // Second sync with different channels
        db.sync_followed(&[
            make_channel("200", "StreamerB"),
            make_channel("300", "StreamerC"),
        ])
        .unwrap();
        let ids = db.get_followed_ids().unwrap();
        assert_eq!(ids.len(), 2);
        assert!(!ids.contains(&100));
        assert!(ids.contains(&200));
        assert!(ids.contains(&300));
    }

    // === ensure_schedule_queue_entries tests ===

    #[test]
    fn ensure_queue_entries_creates_with_zero() {
        let db = in_memory_db();
        db.sync_followed(&[
            make_channel("100", "StreamerA"),
            make_channel("200", "StreamerB"),
        ])
        .unwrap();
        db.ensure_schedule_queue_entries(&[100, 200]).unwrap();

        // Both should be stale (last_checked_at = 0)
        let result = db.get_next_stale_broadcaster(1).unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn ensure_queue_entries_does_not_overwrite() {
        let db = in_memory_db();
        db.sync_followed(&[make_channel("100", "StreamerA")])
            .unwrap();
        db.ensure_schedule_queue_entries(&[100]).unwrap();
        db.update_last_checked(100).unwrap();

        // Re-ensure should not reset the timestamp
        db.ensure_schedule_queue_entries(&[100]).unwrap();

        // Should NOT be stale (was just checked)
        let result = db.get_next_stale_broadcaster(1).unwrap();
        assert!(result.is_none());
    }

    // === get_next_stale_broadcaster tests ===

    #[test]
    fn stale_broadcaster_returns_most_stale() {
        let db = in_memory_db();
        db.sync_followed(&[
            make_channel("100", "StreamerA"),
            make_channel("200", "StreamerB"),
        ])
        .unwrap();
        db.ensure_schedule_queue_entries(&[100, 200]).unwrap();

        // Both have last_checked_at = 0, so the most stale is returned
        let result = db.get_next_stale_broadcaster(24 * 3600).unwrap().unwrap();
        // Should return one of them (both are at 0)
        assert!(result.0 == 100 || result.0 == 200);
    }

    #[test]
    fn no_stale_broadcaster_when_all_fresh() {
        let db = in_memory_db();
        db.sync_followed(&[make_channel("100", "StreamerA")])
            .unwrap();
        db.ensure_schedule_queue_entries(&[100]).unwrap();
        db.update_last_checked(100).unwrap();

        // Threshold of 24h — just checked, so nothing stale
        let result = db.get_next_stale_broadcaster(24 * 3600).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn stale_broadcaster_only_returns_followed() {
        let db = in_memory_db();
        // Add queue entry for 100 but don't add to followed
        db.ensure_schedule_queue_entries(&[100]).unwrap();

        // Should return None since 100 is not in the followed table
        let result = db.get_next_stale_broadcaster(24 * 3600).unwrap();
        assert!(result.is_none());
    }

    // === replace_future_schedules + get_upcoming_schedules tests ===

    fn make_scheduled_stream(
        id: &str,
        broadcaster_id: &str,
        hours_from_now: i64,
    ) -> ScheduledStream {
        ScheduledStream {
            id: id.to_string(),
            broadcaster_id: broadcaster_id.to_string(),
            broadcaster_name: format!("Streamer{}", broadcaster_id),
            broadcaster_login: format!("streamer{}", broadcaster_id),
            title: "Test Schedule".to_string(),
            start_time: Utc::now() + Duration::hours(hours_from_now),
            end_time: None,
            category: Some("Gaming".to_string()),
            category_id: Some("123".to_string()),
            is_recurring: false,
            is_inferred: false,
        }
    }

    #[test]
    fn replace_and_get_schedules() {
        let db = in_memory_db();
        db.sync_followed(&[make_channel("100", "StreamerA")])
            .unwrap();

        let schedules = vec![
            make_scheduled_stream("s1", "100", 2),
            make_scheduled_stream("s2", "100", 5),
        ];
        db.replace_future_schedules(100, &schedules).unwrap();

        let now = Utc::now();
        let start = now - Duration::minutes(15);
        let end = now + Duration::hours(24);
        let upcoming = db.get_upcoming_schedules(start, end).unwrap();
        assert_eq!(upcoming.len(), 2);
        assert_eq!(upcoming[0].broadcaster_id, "100");
        assert!(!upcoming[0].is_inferred);
    }

    #[test]
    fn replace_schedules_clears_old_future() {
        let db = in_memory_db();
        db.sync_followed(&[make_channel("100", "StreamerA")])
            .unwrap();

        let now = Utc::now();
        let start = now - Duration::minutes(15);
        let end = now + Duration::hours(24);

        // First batch
        db.replace_future_schedules(100, &[make_scheduled_stream("s1", "100", 2)])
            .unwrap();
        assert_eq!(db.get_upcoming_schedules(start, end).unwrap().len(), 1);

        // Replace with different schedule
        db.replace_future_schedules(100, &[make_scheduled_stream("s2", "100", 3)])
            .unwrap();
        let upcoming = db.get_upcoming_schedules(start, end).unwrap();
        assert_eq!(upcoming.len(), 1);
        assert_eq!(upcoming[0].id, "s2");
    }

    #[test]
    fn get_upcoming_filters_by_horizon() {
        let db = in_memory_db();
        db.sync_followed(&[make_channel("100", "StreamerA")])
            .unwrap();

        let schedules = vec![
            make_scheduled_stream("s1", "100", 2), // 2h from now — within 24h
            make_scheduled_stream("s2", "100", 30), // 30h from now — outside 24h
        ];
        db.replace_future_schedules(100, &schedules).unwrap();

        let now = Utc::now();
        let start = now - Duration::minutes(15);
        let end = now + Duration::hours(24);
        let upcoming = db.get_upcoming_schedules(start, end).unwrap();
        assert_eq!(upcoming.len(), 1);
        assert_eq!(upcoming[0].id, "s1");
    }

    #[test]
    fn get_upcoming_filters_unfollowed() {
        let db = in_memory_db();
        // Only follow channel 100, not 200
        db.sync_followed(&[make_channel("100", "StreamerA")])
            .unwrap();

        db.replace_future_schedules(100, &[make_scheduled_stream("s1", "100", 2)])
            .unwrap();
        db.replace_future_schedules(200, &[make_scheduled_stream("s2", "200", 2)])
            .unwrap();

        let now = Utc::now();
        let start = now - Duration::minutes(15);
        let end = now + Duration::hours(24);
        let upcoming = db.get_upcoming_schedules(start, end).unwrap();
        assert_eq!(upcoming.len(), 1);
        assert_eq!(upcoming[0].broadcaster_id, "100");
    }

    #[test]
    fn migration_renames_history_db() {
        let dir = tempfile::tempdir().unwrap();
        let old_path = dir.path().join("history.db");
        let new_path = dir.path().join("data.db");

        // Create a real SQLite database at the old path
        let conn = Connection::open(&old_path).unwrap();
        conn.execute_batch("CREATE TABLE test (id INTEGER)")
            .unwrap();
        drop(conn);

        assert!(old_path.exists());
        assert!(!new_path.exists());

        let _db = Database::new(new_path.clone()).unwrap();

        assert!(!old_path.exists());
        assert!(new_path.exists());
    }

    use chrono::Timelike;
}
