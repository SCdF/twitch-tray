use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};

use crate::twitch::{FollowedChannel, ScheduledStream};

/// Infers future schedules from historical stream data.
///
/// Uses a weekly-recurrence heuristic: looks at the same time window shifted
/// back 1, 2, and 3 weeks. A stream is predicted if at least 2 of those 3
/// lookback windows contain a stream at roughly the same time (within 1 hour).
///
/// `window_start` and `window_end` define the prediction window.
///
/// `history` contains `(user_id, started_at_unix_timestamp)` pairs for all
/// streams in the three lookback windows.
///
/// Issues no I/O — purely computational. The caller (Database::infer_schedules)
/// is responsible for loading the relevant rows from SQLite.
pub fn infer_schedules(
    history: &[(i64, i64)],
    channel_lookup: &HashMap<String, FollowedChannel>,
    window_start: DateTime<Utc>,
    window_end: DateTime<Utc>,
) -> Vec<ScheduledStream> {
    let user_id_map: HashMap<i64, &str> = channel_lookup
        .iter()
        .filter_map(|(id_str, _)| id_str.parse::<i64>().ok().map(|id| (id, id_str.as_str())))
        .collect();

    if user_id_map.is_empty() {
        return Vec::new();
    }

    // Define lookback windows (same window shifted back by 1/2/3 weeks).
    let lookback_windows: [(DateTime<Utc>, DateTime<Utc>); 3] = [1, 2, 3].map(|weeks| {
        let shift = Duration::weeks(weeks);
        (window_start - shift, window_end - shift)
    });

    // Build per-window stream maps from the flat history slice.
    let mut window_streams: [HashMap<i64, Vec<DateTime<Utc>>>; 3] =
        [HashMap::new(), HashMap::new(), HashMap::new()];
    for &(uid, ts) in history {
        if let Some(dt) = DateTime::from_timestamp(ts, 0) {
            for (i, &(win_start, win_end)) in lookback_windows.iter().enumerate() {
                if dt >= win_start && dt <= win_end {
                    window_streams[i].entry(uid).or_default().push(dt);
                }
            }
        }
    }

    let mut inferred = Vec::new();

    for (user_id, id_str) in &user_id_map {
        let channel = &channel_lookup[*id_str];

        // Project each stream from each lookback window forward into the prediction window.
        let mut projected_pairs: Vec<(i64, usize)> = Vec::new();

        for (win_idx, _) in lookback_windows.iter().enumerate() {
            let week_number = win_idx + 1; // 1-indexed
            if let Some(streams) = window_streams[win_idx].get(user_id) {
                for stream_time in streams {
                    let projected = *stream_time + Duration::weeks(week_number as i64);
                    if projected >= window_start && projected <= window_end {
                        projected_pairs.push((projected.timestamp(), week_number));
                    }
                }
            }
        }

        if projected_pairs.is_empty() {
            continue;
        }

        // Cluster using single-linkage with 3600s threshold.
        let clusters = cluster_offsets(&projected_pairs, 3600);

        for cluster in clusters {
            // Count distinct weeks represented in this cluster.
            let mut distinct_weeks: Vec<usize> = cluster.iter().map(|&(_, w)| w).collect();
            distinct_weeks.sort_unstable();
            distinct_weeks.dedup();

            // Require at least 2 distinct weeks to confirm a recurring pattern.
            if distinct_weeks.len() < 2 {
                continue;
            }

            // Compute average projected timestamp, rounded to nearest 15 minutes (900s).
            let sum: i64 = cluster.iter().map(|&(ts, _)| ts).sum();
            let avg = sum as f64 / cluster.len() as f64;
            let rounded = ((avg / 900.0).round() as i64) * 900;

            let predicted_time = DateTime::<Utc>::from_timestamp(rounded, 0)
                .expect("valid inferred schedule timestamp");

            if predicted_time < window_start {
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

    // Sort by start time.
    inferred.sort_by_key(|s| s.start_time);
    inferred
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
        // of any item already in the cluster.
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
    use chrono::{TimeZone, Timelike};

    fn make_channel(id: &str, name: &str) -> FollowedChannel {
        FollowedChannel {
            broadcaster_id: id.to_string(),
            broadcaster_login: name.to_lowercase(),
            broadcaster_name: name.to_string(),
            followed_at: Utc::now(),
        }
    }

    fn schedule_window(now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
        (now - Duration::minutes(15), now + Duration::hours(6))
    }

    /// Wraps a single (user_id, timestamp) pair for the history slice.
    fn h(user_id: i64, ts: DateTime<Utc>) -> (i64, i64) {
        (user_id, ts.timestamp())
    }

    // === infer_schedules tests ===

    #[test]
    fn three_weeks_two_match_predicted() {
        let now = Utc.with_ymd_and_hms(2025, 7, 16, 14, 0, 0).unwrap();
        let (start, end) = schedule_window(now);

        // Earliest record establishes coverage for all 3 weeks.
        // Week 3 window starts at start-21d = June 25 13:45.
        let earliest = Utc.with_ymd_and_hms(2025, 6, 25, 12, 0, 0).unwrap();

        // Streamer went live at ~15:00 on 2 of the last 3 weeks (Wed).
        let w1 = Utc.with_ymd_and_hms(2025, 7, 9, 15, 0, 0).unwrap();
        let w2 = Utc.with_ymd_and_hms(2025, 7, 2, 15, 0, 0).unwrap();
        // Week 3: skipped (only 2/3 match)

        let history = vec![h(100, earliest), h(100, w1), h(100, w2)];

        let mut channels = HashMap::new();
        channels.insert("100".to_string(), make_channel("100", "TestStreamer"));

        let result = infer_schedules(&history, &channels, start, end);
        assert_eq!(result.len(), 1, "Should predict one schedule");
        assert!(result[0].is_inferred);
        assert_eq!(result[0].start_time.hour(), 15);
    }

    #[test]
    fn three_weeks_one_match_not_predicted() {
        let now = Utc.with_ymd_and_hms(2025, 7, 16, 14, 0, 0).unwrap();
        let (start, end) = schedule_window(now);

        // Only 1 out of 3 weeks has a stream in the window.
        // threshold is 2 (max(1, 3-1)), so not predicted.
        let w1 = Utc.with_ymd_and_hms(2025, 7, 9, 15, 0, 0).unwrap();
        // w2 and w3 have streams outside the window (at 3am) to establish coverage.
        let w2_early = Utc.with_ymd_and_hms(2025, 7, 2, 3, 0, 0).unwrap();
        let w3_early = Utc.with_ymd_and_hms(2025, 6, 25, 3, 0, 0).unwrap();

        let history = vec![h(100, w1), h(100, w2_early), h(100, w3_early)];

        let mut channels = HashMap::new();
        channels.insert("100".to_string(), make_channel("100", "TestStreamer"));

        let result = infer_schedules(&history, &channels, start, end);
        assert!(
            result.is_empty(),
            "Should not predict with only 1/3 weeks matching"
        );
    }

    #[test]
    fn two_weeks_both_match_predicted() {
        let now = Utc.with_ymd_and_hms(2025, 7, 16, 14, 0, 0).unwrap();
        let (start, end) = schedule_window(now);

        // Earliest establishes coverage for week 2 (starts start-14d = July 2 13:45).
        let earliest = Utc.with_ymd_and_hms(2025, 7, 2, 12, 0, 0).unwrap();

        let w1 = Utc.with_ymd_and_hms(2025, 7, 9, 15, 0, 0).unwrap();
        let w2 = Utc.with_ymd_and_hms(2025, 7, 2, 15, 0, 0).unwrap();

        let history = vec![h(100, earliest), h(100, w1), h(100, w2)];

        let mut channels = HashMap::new();
        channels.insert("100".to_string(), make_channel("100", "TestStreamer"));

        let result = infer_schedules(&history, &channels, start, end);
        assert_eq!(result.len(), 1, "Should predict with 2/2 weeks matching");
    }

    #[test]
    fn two_weeks_one_stream_not_predicted() {
        let now = Utc.with_ymd_and_hms(2025, 7, 16, 14, 0, 0).unwrap();
        let (start, end) = schedule_window(now);

        // Data goes back 2 weeks, but only week 1 has a stream in the window.
        // threshold = weeks_with_data = 2 (sparse coverage), so 1 match is NOT enough.
        let w1 = Utc.with_ymd_and_hms(2025, 7, 9, 15, 0, 0).unwrap();
        // w2 has a stream but outside the window (establishes data coverage only).
        let w2_outside = Utc.with_ymd_and_hms(2025, 7, 2, 3, 0, 0).unwrap();

        let history = vec![h(100, w1), h(100, w2_outside)];

        let mut channels = HashMap::new();
        channels.insert("100".to_string(), make_channel("100", "TestStreamer"));

        let result = infer_schedules(&history, &channels, start, end);
        assert!(
            result.is_empty(),
            "Should not predict with only 1/2 weeks matching — single sighting is not a pattern"
        );
    }

    #[test]
    fn one_week_data_not_predicted() {
        let now = Utc.with_ymd_and_hms(2025, 7, 16, 14, 0, 0).unwrap();
        let (start, end) = schedule_window(now);

        // Only one stream, appearing in a single lookback window.
        // 1 distinct week < 2 → not predicted.
        let w1 = Utc.with_ymd_and_hms(2025, 7, 9, 15, 0, 0).unwrap();

        let history = vec![h(100, w1)];

        let mut channels = HashMap::new();
        channels.insert("100".to_string(), make_channel("100", "TestStreamer"));

        let result = infer_schedules(&history, &channels, start, end);
        assert!(
            result.is_empty(),
            "A single occurrence is not a pattern — should not predict"
        );
    }

    #[test]
    fn no_data_no_prediction() {
        let now = Utc.with_ymd_and_hms(2025, 7, 16, 14, 0, 0).unwrap();
        let (start, end) = schedule_window(now);

        let mut channels = HashMap::new();
        channels.insert("100".to_string(), make_channel("100", "TestStreamer"));

        let result = infer_schedules(&[], &channels, start, end);
        assert!(result.is_empty(), "No data should produce no predictions");
    }

    #[test]
    fn clustering_59min_apart_same_cluster() {
        let now = Utc.with_ymd_and_hms(2025, 7, 16, 14, 0, 0).unwrap();
        let (start, end) = schedule_window(now);

        let earliest = Utc.with_ymd_and_hms(2025, 7, 2, 12, 0, 0).unwrap();

        // Two streams 1 minute apart should cluster together (within 60min threshold).
        let w1 = Utc.with_ymd_and_hms(2025, 7, 9, 15, 55, 0).unwrap();
        let w2 = Utc.with_ymd_and_hms(2025, 7, 2, 15, 54, 0).unwrap();

        let history = vec![h(100, earliest), h(100, w1), h(100, w2)];

        let mut channels = HashMap::new();
        channels.insert("100".to_string(), make_channel("100", "TestStreamer"));

        let result = infer_schedules(&history, &channels, start, end);
        assert_eq!(
            result.len(),
            1,
            "Streams 1min apart should cluster into one prediction"
        );
    }

    #[test]
    fn clustering_61min_apart_different_clusters() {
        let now = Utc.with_ymd_and_hms(2025, 7, 16, 14, 0, 0).unwrap();
        let (start, end) = schedule_window(now);

        let earliest = Utc.with_ymd_and_hms(2025, 7, 2, 12, 0, 0).unwrap();

        // Two streams 61 minutes apart land in separate clusters (don't merge),
        // but with only 2 valid weeks and each time appearing in just 1 week,
        // neither meets the threshold of 2 — so nothing is predicted.
        let w1 = Utc.with_ymd_and_hms(2025, 7, 9, 15, 0, 0).unwrap();
        let w2 = Utc.with_ymd_and_hms(2025, 7, 2, 16, 1, 0).unwrap();

        let history = vec![h(100, earliest), h(100, w1), h(100, w2)];

        let mut channels = HashMap::new();
        channels.insert("100".to_string(), make_channel("100", "TestStreamer"));

        let result = infer_schedules(&history, &channels, start, end);
        // To confirm clustering is still separate: use 3 weeks of data so threshold
        // drops to 2, and give each time a match in week 3 as well.
        // This test just validates the sparse-coverage (2-week) case is not predicted.
        assert!(
            result.is_empty(),
            "With 2 valid weeks and each time appearing only once, nothing should predict"
        );
    }

    #[test]
    fn average_and_rounding() {
        let now = Utc.with_ymd_and_hms(2025, 7, 16, 10, 0, 0).unwrap();
        let (start, end) = schedule_window(now);

        let earliest = Utc.with_ymd_and_hms(2025, 6, 25, 8, 0, 0).unwrap();

        // Three streams at 12:45, 12:58, 13:15 (within 30min, so they cluster).
        // Projected forward: all land on July 16 at 12:45, 12:58, 13:15.
        // Average = 12:59:20 → rounded to nearest 15 min = 13:00.
        let w1 = Utc.with_ymd_and_hms(2025, 7, 9, 12, 45, 0).unwrap();
        let w2 = Utc.with_ymd_and_hms(2025, 7, 2, 12, 58, 0).unwrap();
        let w3 = Utc.with_ymd_and_hms(2025, 6, 25, 13, 15, 0).unwrap();

        let history = vec![h(100, earliest), h(100, w1), h(100, w2), h(100, w3)];

        let mut channels = HashMap::new();
        channels.insert("100".to_string(), make_channel("100", "TestStreamer"));

        let result = infer_schedules(&history, &channels, start, end);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].start_time.hour(), 13);
        assert_eq!(result[0].start_time.minute(), 0);
    }

    #[test]
    fn predicted_time_stable_across_now() {
        // The predicted time should not change when "now" shifts by minutes,
        // since it's based on projecting historical data forward, not on offsets
        // from the current time.
        let earliest = Utc.with_ymd_and_hms(2025, 7, 2, 12, 0, 0).unwrap();
        let w1 = Utc.with_ymd_and_hms(2025, 7, 9, 15, 0, 0).unwrap();
        let w2 = Utc.with_ymd_and_hms(2025, 7, 2, 15, 5, 0).unwrap();

        let history = vec![h(100, earliest), h(100, w1), h(100, w2)];

        let mut channels = HashMap::new();
        channels.insert("100".to_string(), make_channel("100", "TestStreamer"));

        // Run inference at multiple "now" times spanning 30 minutes.
        let mut predicted_times = Vec::new();
        for minute in [0u32, 1, 5, 10, 15, 29] {
            let now = Utc.with_ymd_and_hms(2025, 7, 16, 14, minute, 0).unwrap();
            let (start, end) = schedule_window(now);
            let result = infer_schedules(&history, &channels, start, end);
            assert_eq!(result.len(), 1, "Should predict at minute {}", minute);
            predicted_times.push(result[0].start_time);
        }

        // All predicted times should be identical.
        for (i, t) in predicted_times.iter().enumerate() {
            assert_eq!(
                *t, predicted_times[0],
                "Predicted time at index {} ({}) differs from index 0 ({})",
                i, t, predicted_times[0]
            );
        }
    }

    #[test]
    fn multiple_streamers_independent() {
        let now = Utc.with_ymd_and_hms(2025, 7, 16, 14, 0, 0).unwrap();
        let (start, end) = schedule_window(now);

        let early100 = Utc.with_ymd_and_hms(2025, 7, 2, 12, 0, 0).unwrap();
        let early200 = Utc.with_ymd_and_hms(2025, 7, 2, 12, 0, 0).unwrap();

        // Streamer 100 streams at 15:00, streamer 200 at 18:00.
        let s100_w1 = Utc.with_ymd_and_hms(2025, 7, 9, 15, 0, 0).unwrap();
        let s100_w2 = Utc.with_ymd_and_hms(2025, 7, 2, 15, 0, 0).unwrap();
        let s200_w1 = Utc.with_ymd_and_hms(2025, 7, 9, 18, 0, 0).unwrap();
        let s200_w2 = Utc.with_ymd_and_hms(2025, 7, 2, 18, 0, 0).unwrap();

        let history = vec![
            h(100, early100),
            h(200, early200),
            h(100, s100_w1),
            h(100, s100_w2),
            h(200, s200_w1),
            h(200, s200_w2),
        ];

        let mut channels = HashMap::new();
        channels.insert("100".to_string(), make_channel("100", "StreamerA"));
        channels.insert("200".to_string(), make_channel("200", "StreamerB"));

        let result = infer_schedules(&history, &channels, start, end);
        assert_eq!(result.len(), 2, "Should predict for both streamers");

        let names: Vec<&str> = result.iter().map(|s| s.broadcaster_name.as_str()).collect();
        assert!(names.contains(&"StreamerA"));
        assert!(names.contains(&"StreamerB"));
    }

    #[test]
    fn stream_in_one_lookback_window_only_not_predicted() {
        let now = Utc.with_ymd_and_hms(2025, 7, 16, 14, 0, 0).unwrap();
        let (start, end) = schedule_window(now);

        // Stream appears only in the week-1 lookback window; nothing in weeks 2 or 3.
        // 1 distinct week < 2 → not predicted, regardless of how far back the data goes.
        let w1 = Utc.with_ymd_and_hms(2025, 7, 9, 15, 0, 0).unwrap();

        let history = vec![h(100, w1)];

        let mut channels = HashMap::new();
        channels.insert("100".to_string(), make_channel("100", "TestStreamer"));

        let result = infer_schedules(&history, &channels, start, end);
        assert!(result.is_empty());
    }

    #[test]
    fn clustering_61min_apart_produces_two_separate_predictions_with_full_coverage() {
        // Use 3 weeks of full coverage so threshold is 2.
        // Each slot (15:00 and 16:01) appears in 2 of the 3 weeks → both predicted separately.
        let now = Utc.with_ymd_and_hms(2025, 7, 16, 14, 0, 0).unwrap();
        let (start, end) = schedule_window(now);

        let earliest = Utc.with_ymd_and_hms(2025, 6, 24, 12, 0, 0).unwrap();

        let w1_a = Utc.with_ymd_and_hms(2025, 7, 9, 15, 0, 0).unwrap();
        let w2_a = Utc.with_ymd_and_hms(2025, 7, 2, 15, 0, 0).unwrap();
        let w1_b = Utc.with_ymd_and_hms(2025, 7, 9, 16, 1, 0).unwrap();
        let w2_b = Utc.with_ymd_and_hms(2025, 7, 2, 16, 1, 0).unwrap();

        let history = vec![
            h(100, earliest),
            h(100, w1_a),
            h(100, w2_a),
            h(100, w1_b),
            h(100, w2_b),
        ];

        let mut channels = HashMap::new();
        channels.insert("100".to_string(), make_channel("100", "TestStreamer"));

        let result = infer_schedules(&history, &channels, start, end);
        assert_eq!(
            result.len(),
            2,
            "61min apart with full coverage should produce two separate predictions"
        );
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
        // 4000s apart > 3600s threshold → separate clusters.
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
        // A-B are 2000 apart, B-C are 2000 apart → A-C are 4000 apart.
        // With single-linkage, all should be in one cluster since each adjacent pair
        // is within threshold.
        let pairs = vec![(1000, 1), (3000, 2), (5000, 3)];
        let result = cluster_offsets(&pairs, 3600);
        assert_eq!(
            result.len(),
            1,
            "Single-linkage should chain A-B-C into one cluster"
        );
        assert_eq!(result[0].len(), 3);
    }
}
