/// A single recorded viewer count observation.
#[derive(Debug, Clone)]
pub struct ViewerObservation {
    pub broadcaster_id: i64,
    pub observed_at: i64,
    pub stream_age_min: i64,
    pub viewer_count: u32,
    pub stream_started_at: i64,
}

/// Precomputed stats for a stream-age window.
#[derive(Debug, Clone)]
pub struct BucketStats {
    pub mean: f64,
    pub stddev: f64,
    pub count: usize,
    pub distinct_streams: usize,
}

/// Hotness assessment for a single stream.
#[derive(Debug, Clone)]
pub struct HotnessInfo {
    pub broadcaster_id: String,
    pub z_score: f64,
    pub is_hot: bool,
    pub mean_viewers: f64,
    pub stddev: f64,
    pub current_viewers: u32,
    pub observation_count: usize,
    pub distinct_streams: usize,
}

/// Configuration for hotness detection.
#[derive(Debug, Clone)]
pub struct HotnessConfig {
    pub z_threshold: f64,
    pub min_observations: usize,
    pub min_streams: usize,
}

/// Computes the stream-age window around a given age point.
///
/// The half-width is `max(stream_age_min / 3, 5)` minutes. The lower bound is
/// clamped to zero so early-stream observations are not lost.
pub fn compute_age_window(stream_age_min: i64) -> (i64, i64) {
    let half_width = (stream_age_min / 3).max(5);
    let lower = (stream_age_min - half_width).max(0);
    let upper = stream_age_min + half_width;
    (lower, upper)
}

/// Computes mean and population standard deviation from viewer counts.
///
/// Empty input yields count=0, mean=0.0, stddev=0.0, distinct_streams=0.
pub fn compute_bucket_stats(observations: &[ViewerObservation]) -> BucketStats {
    if observations.is_empty() {
        return BucketStats {
            mean: 0.0,
            stddev: 0.0,
            count: 0,
            distinct_streams: 0,
        };
    }

    let count = observations.len();
    let sum: f64 = observations.iter().map(|o| f64::from(o.viewer_count)).sum();
    let mean = sum / count as f64;

    let variance = observations
        .iter()
        .map(|o| {
            let diff = f64::from(o.viewer_count) - mean;
            diff * diff
        })
        .sum::<f64>()
        / count as f64;

    let distinct_streams: std::collections::HashSet<i64> =
        observations.iter().map(|o| o.stream_started_at).collect();

    BucketStats {
        mean,
        stddev: variance.sqrt(),
        count,
        distinct_streams: distinct_streams.len(),
    }
}

/// Evaluates whether a stream is "hot" based on current viewers and historical bucket stats.
///
/// Returns `None` if there are insufficient observations or zero standard deviation
/// (all historical observations were identical).
pub fn compute_hotness(
    broadcaster_id: &str,
    current_viewers: u32,
    stats: &BucketStats,
    config: &HotnessConfig,
) -> Option<HotnessInfo> {
    if stats.count < config.min_observations
        || stats.distinct_streams < config.min_streams
        || stats.stddev == 0.0
    {
        return None;
    }

    let z_score = (f64::from(current_viewers) - stats.mean) / stats.stddev;

    Some(HotnessInfo {
        broadcaster_id: broadcaster_id.to_string(),
        z_score,
        is_hot: z_score >= config.z_threshold,
        mean_viewers: stats.mean,
        stddev: stats.stddev,
        current_viewers,
        observation_count: stats.count,
        distinct_streams: stats.distinct_streams,
    })
}

/// Precomputes bucket stats for multiple stream-age points.
///
/// For each age point, computes the corresponding age window, filters observations
/// to that window, and computes stats. Returns `(age_point, BucketStats)` pairs.
pub fn compute_hotness_profile(
    observations: &[ViewerObservation],
    age_points: &[i64],
) -> Vec<(i64, BucketStats)> {
    age_points
        .iter()
        .map(|&age| {
            let (lo, hi) = compute_age_window(age);
            let filtered: Vec<_> = observations
                .iter()
                .filter(|o| o.stream_age_min >= lo && o.stream_age_min <= hi)
                .cloned()
                .collect();
            (age, compute_bucket_stats(&filtered))
        })
        .collect()
}

/// Finds the bucket with the closest age point to `stream_age_min`.
///
/// Returns `None` if the profile is empty.
pub fn find_nearest_bucket(
    profile: &[(i64, BucketStats)],
    stream_age_min: i64,
) -> Option<&BucketStats> {
    profile
        .iter()
        .min_by_key(|(age, _)| (age - stream_age_min).unsigned_abs())
        .map(|(_, stats)| stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(broadcaster_id: i64, stream_age_min: i64, viewer_count: u32) -> ViewerObservation {
        obs_stream(broadcaster_id, stream_age_min, viewer_count, 1_000_000)
    }

    fn obs_stream(
        broadcaster_id: i64,
        stream_age_min: i64,
        viewer_count: u32,
        stream_started_at: i64,
    ) -> ViewerObservation {
        ViewerObservation {
            broadcaster_id,
            observed_at: 1_700_000_000,
            stream_age_min,
            viewer_count,
            stream_started_at,
        }
    }

    // === compute_age_window ===

    #[test]
    fn age_window_at_minute_0() {
        // half_width = max(0/3, 5) = 5 → (0-5, 0+5) clamped → (0, 5)
        assert_eq!(compute_age_window(0), (0, 5));
    }

    #[test]
    fn age_window_at_minute_3() {
        // half_width = max(1, 5) = 5 → (3-5, 3+5) clamped → (0, 8)
        assert_eq!(compute_age_window(3), (0, 8));
    }

    #[test]
    fn age_window_at_minute_15() {
        // half_width = max(5, 5) = 5 → (10, 20)
        assert_eq!(compute_age_window(15), (10, 20));
    }

    #[test]
    fn age_window_at_minute_60() {
        // half_width = max(20, 5) = 20 → (40, 80)
        assert_eq!(compute_age_window(60), (40, 80));
    }

    #[test]
    fn age_window_at_minute_180() {
        // half_width = max(60, 5) = 60 → (120, 240)
        assert_eq!(compute_age_window(180), (120, 240));
    }

    // === compute_bucket_stats ===

    #[test]
    fn bucket_stats_empty_observations() {
        let stats = compute_bucket_stats(&[]);
        assert_eq!(stats.count, 0);
        assert!((stats.mean).abs() < f64::EPSILON);
        assert!((stats.stddev).abs() < f64::EPSILON);
    }

    #[test]
    fn bucket_stats_single_observation() {
        let stats = compute_bucket_stats(&[obs(1, 10, 5000)]);
        assert_eq!(stats.count, 1);
        assert!((stats.mean - 5000.0).abs() < f64::EPSILON);
        assert!((stats.stddev).abs() < f64::EPSILON);
    }

    #[test]
    fn bucket_stats_computed_correctly() {
        // Values: 100, 200, 300 → mean=200, variance=((100²+0+100²)/3)=6666.67, stddev≈81.65
        let observations = vec![obs(1, 10, 100), obs(1, 11, 200), obs(1, 12, 300)];
        let stats = compute_bucket_stats(&observations);
        assert_eq!(stats.count, 3);
        assert!((stats.mean - 200.0).abs() < 0.001);
        assert!((stats.stddev - 81.650).abs() < 0.01);
    }

    #[test]
    fn bucket_stats_all_identical() {
        let observations = vec![obs(1, 10, 500), obs(1, 11, 500), obs(1, 12, 500)];
        let stats = compute_bucket_stats(&observations);
        assert_eq!(stats.count, 3);
        assert!((stats.mean - 500.0).abs() < f64::EPSILON);
        assert!((stats.stddev).abs() < f64::EPSILON);
    }

    // === compute_hotness ===

    #[test]
    fn z_score_computed_correctly() {
        let stats = BucketStats {
            mean: 2000.0,
            stddev: 500.0,
            count: 10,
            distinct_streams: 5,
        };
        let config = HotnessConfig {
            z_threshold: 2.0,
            min_observations: 5,
            min_streams: 1,
        };
        let info = compute_hotness("123", 3500, &stats, &config).unwrap();
        assert!((info.z_score - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn hot_when_z_score_exceeds_threshold() {
        let stats = BucketStats {
            mean: 2000.0,
            stddev: 500.0,
            count: 10,
            distinct_streams: 5,
        };
        let config = HotnessConfig {
            z_threshold: 2.0,
            min_observations: 5,
            min_streams: 1,
        };
        let info = compute_hotness("123", 4000, &stats, &config).unwrap();
        assert!(info.is_hot);
        assert_eq!(info.current_viewers, 4000);
        assert!((info.mean_viewers - 2000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn not_hot_when_below_threshold() {
        let stats = BucketStats {
            mean: 2000.0,
            stddev: 500.0,
            count: 10,
            distinct_streams: 5,
        };
        let config = HotnessConfig {
            z_threshold: 2.0,
            min_observations: 5,
            min_streams: 1,
        };
        let info = compute_hotness("123", 2500, &stats, &config).unwrap();
        assert!(!info.is_hot);
    }

    #[test]
    fn not_hot_when_exactly_at_threshold() {
        let stats = BucketStats {
            mean: 2000.0,
            stddev: 500.0,
            count: 10,
            distinct_streams: 5,
        };
        let config = HotnessConfig {
            z_threshold: 2.0,
            min_observations: 5,
            min_streams: 1,
        };
        // 2000 + 2*500 = 3000 → z=2.0, exactly at threshold → is_hot (>=)
        let info = compute_hotness("123", 3000, &stats, &config).unwrap();
        assert!(info.is_hot);
    }

    #[test]
    fn not_hot_when_insufficient_observations() {
        let stats = BucketStats {
            mean: 2000.0,
            stddev: 500.0,
            count: 3,
            distinct_streams: 2,
        };
        let config = HotnessConfig {
            z_threshold: 2.0,
            min_observations: 5,
            min_streams: 1,
        };
        assert!(compute_hotness("123", 5000, &stats, &config).is_none());
    }

    #[test]
    fn not_hot_when_stddev_is_zero() {
        let stats = BucketStats {
            mean: 2000.0,
            stddev: 0.0,
            count: 10,
            distinct_streams: 5,
        };
        let config = HotnessConfig {
            z_threshold: 2.0,
            min_observations: 5,
            min_streams: 1,
        };
        assert!(compute_hotness("123", 5000, &stats, &config).is_none());
    }

    // === compute_hotness_profile ===

    #[test]
    fn profile_precomputes_multiple_age_points() {
        let observations = vec![
            obs(1, 5, 100),
            obs(1, 10, 200),
            obs(1, 30, 1000),
            obs(1, 60, 2000),
        ];
        let profile = compute_hotness_profile(&observations, &[10, 30, 60]);
        assert_eq!(profile.len(), 3);
        assert_eq!(profile[0].0, 10);
        assert_eq!(profile[1].0, 30);
        assert_eq!(profile[2].0, 60);
    }

    #[test]
    fn profile_filters_observations_to_correct_windows() {
        // age_point=60 → window (40, 80)
        // Only obs at 50 and 70 should be included, not obs at 10
        let observations = vec![obs(1, 10, 100), obs(1, 50, 1000), obs(1, 70, 2000)];
        let profile = compute_hotness_profile(&observations, &[60]);
        assert_eq!(profile[0].1.count, 2);
        assert!((profile[0].1.mean - 1500.0).abs() < 0.001);
    }

    // === find_nearest_bucket ===

    #[test]
    fn find_nearest_bucket_selects_closest() {
        let profile = vec![
            (
                10,
                BucketStats {
                    mean: 100.0,
                    stddev: 10.0,
                    count: 5,
                    distinct_streams: 3,
                },
            ),
            (
                30,
                BucketStats {
                    mean: 500.0,
                    stddev: 50.0,
                    count: 5,
                    distinct_streams: 3,
                },
            ),
            (
                60,
                BucketStats {
                    mean: 1000.0,
                    stddev: 100.0,
                    count: 5,
                    distinct_streams: 3,
                },
            ),
        ];
        let stats = find_nearest_bucket(&profile, 25).unwrap();
        assert!((stats.mean - 500.0).abs() < f64::EPSILON);
    }

    #[test]
    fn find_nearest_bucket_empty_profile() {
        let profile: Vec<(i64, BucketStats)> = vec![];
        assert!(find_nearest_bucket(&profile, 30).is_none());
    }

    #[test]
    fn not_hot_when_insufficient_distinct_streams() {
        let stats = BucketStats {
            mean: 2000.0,
            stddev: 500.0,
            count: 50,
            distinct_streams: 2,
        };
        let config = HotnessConfig {
            z_threshold: 2.0,
            min_observations: 5,
            min_streams: 7,
        };
        assert!(compute_hotness("123", 5000, &stats, &config).is_none());
    }

    #[test]
    fn hot_when_sufficient_distinct_streams() {
        let stats = BucketStats {
            mean: 2000.0,
            stddev: 500.0,
            count: 50,
            distinct_streams: 7,
        };
        let config = HotnessConfig {
            z_threshold: 2.0,
            min_observations: 5,
            min_streams: 7,
        };
        let info = compute_hotness("123", 5000, &stats, &config).unwrap();
        assert!(info.is_hot);
    }

    #[test]
    fn bucket_stats_counts_distinct_streams() {
        // 3 observations from 2 distinct streams
        let observations = vec![
            obs_stream(1, 10, 100, 1_000_000),
            obs_stream(1, 11, 200, 1_000_000),
            obs_stream(1, 12, 300, 2_000_000),
        ];
        let stats = compute_bucket_stats(&observations);
        assert_eq!(stats.count, 3);
        assert_eq!(stats.distinct_streams, 2);
    }

    #[test]
    fn bucket_stats_single_stream_has_one_distinct() {
        let observations = vec![obs(1, 10, 100), obs(1, 11, 200), obs(1, 12, 300)];
        let stats = compute_bucket_stats(&observations);
        assert_eq!(stats.distinct_streams, 1);
    }

    #[test]
    fn find_nearest_bucket_exact_match() {
        let profile = vec![
            (
                10,
                BucketStats {
                    mean: 100.0,
                    stddev: 10.0,
                    count: 5,
                    distinct_streams: 3,
                },
            ),
            (
                30,
                BucketStats {
                    mean: 500.0,
                    stddev: 50.0,
                    count: 5,
                    distinct_streams: 3,
                },
            ),
        ];
        let stats = find_nearest_bucket(&profile, 30).unwrap();
        assert!((stats.mean - 500.0).abs() < f64::EPSILON);
    }
}
