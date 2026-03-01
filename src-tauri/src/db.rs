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
    /// Thin wrapper around `schedule_inference::infer_schedules`. Loads streams
    /// from the three lookback windows (3 SQL queries) then delegates to the
    /// pure function.
    pub fn infer_schedules(
        &self,
        channel_lookup: &HashMap<String, FollowedChannel>,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> anyhow::Result<Vec<ScheduledStream>> {
        let all_user_ids: Vec<i64> = channel_lookup
            .keys()
            .filter_map(|id_str| id_str.parse::<i64>().ok())
            .collect();

        if all_user_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut history: Vec<(i64, i64)> = Vec::new();

        for weeks in 1..=3i64 {
            let shift = Duration::weeks(weeks);
            for (uid, timestamps) in
                self.get_streams_in_range(&all_user_ids, start - shift, end - shift)?
            {
                for ts in timestamps {
                    history.push((uid, ts.timestamp()));
                }
            }
        }

        Ok(crate::schedule_inference::infer_schedules(
            &history,
            channel_lookup,
            start,
            end,
        ))
    }

    /// Returns all followed channels as a HashMap keyed by broadcaster_id string.
    pub fn get_followed_channel_lookup(&self) -> anyhow::Result<HashMap<String, FollowedChannel>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT broadcaster_id, broadcaster_login, broadcaster_name, followed_at FROM followed",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;
        let mut result = HashMap::new();
        for row in rows {
            let (id, login, name, followed_at_ts) = row?;
            let channel = FollowedChannel {
                broadcaster_id: id.to_string(),
                broadcaster_login: login,
                broadcaster_name: name,
                followed_at: DateTime::from_timestamp(followed_at_ts, 0).unwrap_or_else(Utc::now),
            };
            result.insert(id.to_string(), channel);
        }
        Ok(result)
    }

    /// Returns raw stream history rows within `[start, end)` for currently-followed channels.
    ///
    /// Returns `(broadcaster_name, broadcaster_login, started_at_unix)` tuples,
    /// ordered by `started_at`. Channels not in the `followed` table are excluded.
    pub fn get_raw_history_in_window(
        &self,
        start: i64,
        end: i64,
    ) -> anyhow::Result<Vec<(String, String, i64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT f.broadcaster_name, f.broadcaster_login, h.started_at
             FROM stream_history h
             JOIN followed f ON h.user_id = f.broadcaster_id
             WHERE h.started_at >= ?1 AND h.started_at < ?2
             ORDER BY h.started_at",
        )?;
        let rows = stmt.query_map(rusqlite::params![start, end], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }
}

/// Generates `count` SQL placeholders: "?,?,?"
fn repeat_vars(count: usize) -> String {
    let mut s = "?,".repeat(count);
    s.pop(); // remove trailing comma
    s
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

    // Inference logic and cluster_offsets are tested in schedule_inference.rs.

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

    // === get_followed_channel_lookup tests ===

    #[test]
    fn channel_lookup_returns_all_followed() {
        let db = in_memory_db();
        db.sync_followed(&[
            make_channel("100", "StreamerA"),
            make_channel("200", "StreamerB"),
        ])
        .unwrap();

        let lookup = db.get_followed_channel_lookup().unwrap();
        assert_eq!(lookup.len(), 2);
        assert!(lookup.contains_key("100"));
        assert!(lookup.contains_key("200"));
        assert_eq!(lookup["100"].broadcaster_name, "StreamerA");
        assert_eq!(lookup["200"].broadcaster_login, "streamerb");
    }

    #[test]
    fn channel_lookup_empty_when_no_followed() {
        let db = in_memory_db();
        let lookup = db.get_followed_channel_lookup().unwrap();
        assert!(lookup.is_empty());
    }

    // === get_raw_history_in_window tests ===

    #[test]
    fn raw_history_returns_entries_within_range() {
        let db = in_memory_db();
        db.sync_followed(&[make_channel("100", "StreamerA")])
            .unwrap();

        let base = Utc.with_ymd_and_hms(2025, 6, 15, 12, 0, 0).unwrap();
        let s1 = make_test_stream("100", base);
        let s2 = make_test_stream("100", base + Duration::hours(1));
        db.record_streams(&[s1, s2]).unwrap();

        let start = base.timestamp();
        let end = (base + Duration::hours(2)).timestamp();
        let rows = db.get_raw_history_in_window(start, end).unwrap();
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn raw_history_excludes_entries_outside_range() {
        let db = in_memory_db();
        db.sync_followed(&[make_channel("100", "StreamerA")])
            .unwrap();

        let base = Utc.with_ymd_and_hms(2025, 6, 15, 12, 0, 0).unwrap();
        // Inside range
        db.record_streams(&[make_test_stream("100", base + Duration::hours(1))])
            .unwrap();
        // Outside range (at end boundary — exclusive)
        db.record_streams(&[make_test_stream("100", base + Duration::hours(3))])
            .unwrap();

        let start = base.timestamp();
        let end = (base + Duration::hours(3)).timestamp(); // exclusive
        let rows = db.get_raw_history_in_window(start, end).unwrap();
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn raw_history_joins_followed_for_broadcaster_names() {
        let db = in_memory_db();
        db.sync_followed(&[make_channel("100", "StreamerA")])
            .unwrap();

        let base = Utc.with_ymd_and_hms(2025, 6, 15, 12, 0, 0).unwrap();
        db.record_streams(&[make_test_stream("100", base)]).unwrap();

        let rows = db
            .get_raw_history_in_window(base.timestamp(), base.timestamp() + 3600)
            .unwrap();
        assert_eq!(rows.len(), 1);
        let (name, login, _ts) = &rows[0];
        assert_eq!(name, "StreamerA");
        assert_eq!(login, "streamera");
    }

    #[test]
    fn raw_history_excludes_unfollowed_channels() {
        let db = in_memory_db();
        // Only follow channel 100, not 200
        db.sync_followed(&[make_channel("100", "StreamerA")])
            .unwrap();

        let base = Utc.with_ymd_and_hms(2025, 6, 15, 12, 0, 0).unwrap();
        db.record_streams(&[make_test_stream("100", base)]).unwrap();
        // Record history for unfollowed channel 200
        db.record_streams(&[make_test_stream("200", base)]).unwrap();

        let rows = db
            .get_raw_history_in_window(base.timestamp(), base.timestamp() + 3600)
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1, "streamera"); // login of followed channel
    }

    #[test]
    fn raw_history_empty_when_no_data() {
        let db = in_memory_db();
        db.sync_followed(&[make_channel("100", "StreamerA")])
            .unwrap();

        let rows = db.get_raw_history_in_window(0, 9_999_999_999).unwrap();
        assert!(rows.is_empty());
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
}
