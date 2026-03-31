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
    /// Mean of Anscombe-transformed viewer counts (variance-stabilized).
    pub transformed_mean: f64,
    /// Stddev of Anscombe-transformed viewer counts (variance-stabilized).
    pub transformed_stddev: f64,
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
    /// Z-score below which a hot stream is considered "cooled off" (hysteresis).
    /// Must be less than `z_threshold` to prevent oscillation at the boundary.
    pub z_cool_threshold: f64,
    pub min_observations: usize,
    pub min_streams: usize,
}

/// Anscombe variance-stabilizing transform for Poisson-like count data.
///
/// Maps `x` to `sqrt(x + 3/8)`, making the variance approximately constant (~1/4)
/// regardless of the mean. This ensures z-scores are comparable across streamers
/// with very different viewer counts.
fn anscombe(x: f64) -> f64 {
    (x + 0.375).sqrt()
}

/// Computes the stream-age window around a given age point.
///
/// The half-width is `max(stream_age_min / divisor, 5)` minutes. The lower bound is
/// clamped to zero so early-stream observations are not lost.
///
/// A `divisor` of 0 is treated as 1 (the full stream age) to avoid division by zero.
pub fn compute_age_window(stream_age_min: i64, divisor: u32) -> (i64, i64) {
    let d = (divisor.max(1)) as i64;
    let half_width = (stream_age_min / d).max(5);
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
            transformed_mean: 0.0,
            transformed_stddev: 0.0,
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

    let transformed_sum: f64 = observations
        .iter()
        .map(|o| anscombe(f64::from(o.viewer_count)))
        .sum();
    let transformed_mean = transformed_sum / count as f64;

    let transformed_variance = observations
        .iter()
        .map(|o| {
            let diff = anscombe(f64::from(o.viewer_count)) - transformed_mean;
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
        transformed_mean,
        transformed_stddev: transformed_variance.sqrt(),
    }
}

/// Evaluates whether a stream is "hot" based on current viewers and historical bucket stats.
///
/// Uses hysteresis (Schmitt trigger) to prevent oscillation at the threshold boundary:
/// - A stream becomes hot when z-score >= `z_threshold`
/// - A hot stream cools off only when z-score < `z_cool_threshold`
/// - Between the two thresholds, the previous state is preserved
///
/// `was_hot` indicates whether the stream was hot on the previous evaluation.
///
/// Returns `None` if there are insufficient observations or zero standard deviation
/// (all historical observations were identical).
pub fn compute_hotness(
    broadcaster_id: &str,
    current_viewers: u32,
    stats: &BucketStats,
    config: &HotnessConfig,
    was_hot: bool,
) -> Option<HotnessInfo> {
    if stats.count < config.min_observations
        || stats.distinct_streams < config.min_streams
        || stats.transformed_stddev == 0.0
    {
        return None;
    }

    let z_score =
        (anscombe(f64::from(current_viewers)) - stats.transformed_mean) / stats.transformed_stddev;

    let is_hot = if was_hot {
        // Already hot — stay hot unless z drops below cool threshold
        z_score >= config.z_cool_threshold
    } else {
        // Not hot — only become hot if z exceeds entry threshold
        z_score >= config.z_threshold
    };

    Some(HotnessInfo {
        broadcaster_id: broadcaster_id.to_string(),
        z_score,
        is_hot,
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
    age_window_divisor: u32,
) -> Vec<(i64, BucketStats)> {
    age_points
        .iter()
        .map(|&age| {
            let (lo, hi) = compute_age_window(age, age_window_divisor);
            let filtered: Vec<_> = observations
                .iter()
                .filter(|o| o.stream_age_min >= lo && o.stream_age_min <= hi)
                .cloned()
                .collect();
            (age, compute_bucket_stats(&filtered))
        })
        .collect()
}

/// Returns whether a stream's age is within the hotness evaluation window.
///
/// If `max_stream_age_min` is 0, the window is infinite (always evaluate).
/// Otherwise, the stream must be at most `max_stream_age_min` minutes old.
pub fn is_within_hotness_window(stream_age_min: i64, max_stream_age_min: u64) -> bool {
    max_stream_age_min == 0 || stream_age_min <= max_stream_age_min as i64
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
    fn age_window_at_minute_0_divisor_2() {
        // half_width = max(0/2, 5) = 5 → (0-5, 0+5) clamped → (0, 5)
        assert_eq!(compute_age_window(0, 2), (0, 5));
    }

    #[test]
    fn age_window_at_minute_3_divisor_2() {
        // half_width = max(1, 5) = 5 → (3-5, 3+5) clamped → (0, 8)
        assert_eq!(compute_age_window(3, 2), (0, 8));
    }

    #[test]
    fn age_window_at_minute_15_divisor_2() {
        // half_width = max(7, 5) = 7 → (8, 22)
        assert_eq!(compute_age_window(15, 2), (8, 22));
    }

    #[test]
    fn age_window_at_minute_60_divisor_2() {
        // half_width = max(30, 5) = 30 → (30, 90)
        assert_eq!(compute_age_window(60, 2), (30, 90));
    }

    #[test]
    fn age_window_at_minute_180_divisor_2() {
        // half_width = max(90, 5) = 90 → (90, 270)
        assert_eq!(compute_age_window(180, 2), (90, 270));
    }

    #[test]
    fn age_window_divisor_3_gives_narrower_window() {
        // half_width = max(60/3, 5) = 20 → (40, 80)
        assert_eq!(compute_age_window(60, 3), (40, 80));
    }

    #[test]
    fn age_window_divisor_0_treated_as_1() {
        // divisor 0 clamped to 1 → half_width = max(60/1, 5) = 60 → (0, 120)
        assert_eq!(compute_age_window(60, 0), (0, 120));
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
    fn z_score_computed_in_transformed_space() {
        let stats = BucketStats {
            mean: 2000.0,
            stddev: 500.0,
            count: 10,
            distinct_streams: 5,
            transformed_mean: 44.0,
            transformed_stddev: 5.0,
        };
        let config = HotnessConfig {
            z_threshold: 2.0,
            z_cool_threshold: 1.0,
            min_observations: 5,
            min_streams: 1,
        };
        let info = compute_hotness("123", 3500, &stats, &config, false).unwrap();
        // z = (anscombe(3500) - 44.0) / 5.0
        let expected_z = ((3500.0_f64 + 0.375).sqrt() - 44.0) / 5.0;
        assert!((info.z_score - expected_z).abs() < f64::EPSILON);
    }

    #[test]
    fn hot_when_z_score_exceeds_threshold() {
        // transformed_mean=50, transformed_stddev=5
        // anscombe(4000) ≈ 63.25, z ≈ (63.25-50)/5 ≈ 2.65 → hot
        let stats = BucketStats {
            mean: 2000.0,
            stddev: 500.0,
            count: 10,
            distinct_streams: 5,
            transformed_mean: 50.0,
            transformed_stddev: 5.0,
        };
        let config = HotnessConfig {
            z_threshold: 2.0,
            z_cool_threshold: 1.0,
            min_observations: 5,
            min_streams: 1,
        };
        let info = compute_hotness("123", 4000, &stats, &config, false).unwrap();
        assert!(info.is_hot);
        assert_eq!(info.current_viewers, 4000);
        assert!((info.mean_viewers - 2000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn not_hot_when_below_threshold() {
        // anscombe(2500) ≈ 50.00, z ≈ (50-50)/5 ≈ 0 → not hot
        let stats = BucketStats {
            mean: 2000.0,
            stddev: 500.0,
            count: 10,
            distinct_streams: 5,
            transformed_mean: 50.0,
            transformed_stddev: 5.0,
        };
        let config = HotnessConfig {
            z_threshold: 2.0,
            z_cool_threshold: 1.0,
            min_observations: 5,
            min_streams: 1,
        };
        let info = compute_hotness("123", 2500, &stats, &config, false).unwrap();
        assert!(!info.is_hot);
    }

    #[test]
    fn hot_at_or_above_threshold() {
        // Pick transformed values so we can hit exactly z=2.0:
        // anscombe(v) = transformed_mean + z * transformed_stddev
        // We want z >= 2.0 for some integer v.
        // transformed_mean=40, transformed_stddev=10 → need anscombe(v) >= 60 → v >= 3600-0.375
        // anscombe(3600) = sqrt(3600.375) ≈ 60.003 → z ≈ 2.0003 → is_hot (>=)
        let stats = BucketStats {
            mean: 1500.0,
            stddev: 400.0,
            count: 10,
            distinct_streams: 5,
            transformed_mean: 40.0,
            transformed_stddev: 10.0,
        };
        let config = HotnessConfig {
            z_threshold: 2.0,
            z_cool_threshold: 1.0,
            min_observations: 5,
            min_streams: 1,
        };
        let info = compute_hotness("123", 3600, &stats, &config, false).unwrap();
        assert!(info.is_hot);
    }

    #[test]
    fn not_hot_when_insufficient_observations() {
        let stats = BucketStats {
            mean: 2000.0,
            stddev: 500.0,
            count: 3,
            distinct_streams: 2,
            transformed_mean: 44.0,
            transformed_stddev: 5.0,
        };
        let config = HotnessConfig {
            z_threshold: 2.0,
            z_cool_threshold: 1.0,
            min_observations: 5,
            min_streams: 1,
        };
        assert!(compute_hotness("123", 5000, &stats, &config, false).is_none());
    }

    #[test]
    fn not_hot_when_transformed_stddev_is_zero() {
        let stats = BucketStats {
            mean: 2000.0,
            stddev: 0.0,
            count: 10,
            distinct_streams: 5,
            transformed_mean: 44.726,
            transformed_stddev: 0.0,
        };
        let config = HotnessConfig {
            z_threshold: 2.0,
            z_cool_threshold: 1.0,
            min_observations: 5,
            min_streams: 1,
        };
        assert!(compute_hotness("123", 5000, &stats, &config, false).is_none());
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
        let profile = compute_hotness_profile(&observations, &[10, 30, 60], 2);
        assert_eq!(profile.len(), 3);
        assert_eq!(profile[0].0, 10);
        assert_eq!(profile[1].0, 30);
        assert_eq!(profile[2].0, 60);
    }

    #[test]
    fn profile_filters_observations_to_correct_windows() {
        // divisor=2, age_point=60 → half_width=30 → window (30, 90)
        // obs at 50 and 70 are in window, obs at 10 is outside
        let observations = vec![obs(1, 10, 100), obs(1, 50, 1000), obs(1, 70, 2000)];
        let profile = compute_hotness_profile(&observations, &[60], 2);
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
                    transformed_mean: 0.0,
                    transformed_stddev: 0.0,
                },
            ),
            (
                30,
                BucketStats {
                    mean: 500.0,
                    stddev: 50.0,
                    count: 5,
                    distinct_streams: 3,
                    transformed_mean: 0.0,
                    transformed_stddev: 0.0,
                },
            ),
            (
                60,
                BucketStats {
                    mean: 1000.0,
                    stddev: 100.0,
                    count: 5,
                    distinct_streams: 3,
                    transformed_mean: 0.0,
                    transformed_stddev: 0.0,
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
            transformed_mean: 44.0,
            transformed_stddev: 5.0,
        };
        let config = HotnessConfig {
            z_threshold: 2.0,
            z_cool_threshold: 1.0,
            min_observations: 5,
            min_streams: 7,
        };
        assert!(compute_hotness("123", 5000, &stats, &config, false).is_none());
    }

    #[test]
    fn hot_when_sufficient_distinct_streams() {
        // anscombe(5000) ≈ 70.71, z ≈ (70.71-44)/5 ≈ 5.34 → hot
        let stats = BucketStats {
            mean: 2000.0,
            stddev: 500.0,
            count: 50,
            distinct_streams: 7,
            transformed_mean: 44.0,
            transformed_stddev: 5.0,
        };
        let config = HotnessConfig {
            z_threshold: 2.0,
            z_cool_threshold: 1.0,
            min_observations: 5,
            min_streams: 7,
        };
        let info = compute_hotness("123", 5000, &stats, &config, false).unwrap();
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
                    transformed_mean: 0.0,
                    transformed_stddev: 0.0,
                },
            ),
            (
                30,
                BucketStats {
                    mean: 500.0,
                    stddev: 50.0,
                    count: 5,
                    distinct_streams: 3,
                    transformed_mean: 0.0,
                    transformed_stddev: 0.0,
                },
            ),
        ];
        let stats = find_nearest_bucket(&profile, 30).unwrap();
        assert!((stats.mean - 500.0).abs() < f64::EPSILON);
    }

    #[test]
    fn anscombe_transform_stabilizes_variance_across_scales() {
        // Small streamer: avg ~10 viewers, current 16 (+60%)
        // Large streamer: avg ~10000 viewers, current 16000 (+60%)
        // Without transform, small streamer would have inflated z-score.
        // With Anscombe, proportionally similar spikes produce similar z-scores.
        let small_obs: Vec<_> = (0..30)
            .map(|i| obs_stream(1, 10, 8 + (i % 5), i as i64))
            .collect();
        let large_obs: Vec<_> = (0..30)
            .map(|i| obs_stream(2, 10, 8000 + (i % 5) * 1000, i as i64))
            .collect();

        let small_stats = compute_bucket_stats(&small_obs);
        let large_stats = compute_bucket_stats(&large_obs);

        let config = HotnessConfig {
            z_threshold: 2.0,
            z_cool_threshold: 1.0,
            min_observations: 5,
            min_streams: 7,
        };

        let small_info = compute_hotness("1", 16, &small_stats, &config, false).unwrap();
        let large_info = compute_hotness("2", 16000, &large_stats, &config, false).unwrap();

        // The z-scores should be in the same ballpark (both ~60% above mean)
        // rather than the small streamer having a dramatically higher z-score
        assert!(
            (small_info.z_score - large_info.z_score).abs() < 2.0,
            "z-scores should be comparable: small={:.2}, large={:.2}",
            small_info.z_score,
            large_info.z_score
        );
    }

    // === hysteresis (Schmitt trigger) ===

    #[test]
    fn hot_stream_stays_hot_in_dead_zone() {
        // z ≈ 1.5 — between cool threshold (1.0) and entry threshold (2.0)
        // was_hot=true → should remain hot
        let stats = BucketStats {
            mean: 2000.0,
            stddev: 500.0,
            count: 10,
            distinct_streams: 5,
            transformed_mean: 40.0,
            transformed_stddev: 10.0,
        };
        let config = HotnessConfig {
            z_threshold: 2.0,
            z_cool_threshold: 1.0,
            min_observations: 5,
            min_streams: 1,
        };
        // anscombe(v) = 40 + 1.5*10 = 55 → v = 55²-0.375 ≈ 3024.625
        let info = compute_hotness("123", 3025, &stats, &config, true).unwrap();
        assert!(
            info.is_hot,
            "z={:.2} should stay hot (above cool threshold)",
            info.z_score
        );
    }

    #[test]
    fn cold_stream_stays_cold_in_dead_zone() {
        // Same z ≈ 1.5 but was_hot=false → should stay cold
        let stats = BucketStats {
            mean: 2000.0,
            stddev: 500.0,
            count: 10,
            distinct_streams: 5,
            transformed_mean: 40.0,
            transformed_stddev: 10.0,
        };
        let config = HotnessConfig {
            z_threshold: 2.0,
            z_cool_threshold: 1.0,
            min_observations: 5,
            min_streams: 1,
        };
        let info = compute_hotness("123", 3025, &stats, &config, false).unwrap();
        assert!(
            !info.is_hot,
            "z={:.2} should stay cold (below entry threshold)",
            info.z_score
        );
    }

    #[test]
    fn hot_stream_cools_off_below_cool_threshold() {
        // z < 1.0 and was_hot=true → should become not hot
        let stats = BucketStats {
            mean: 2000.0,
            stddev: 500.0,
            count: 10,
            distinct_streams: 5,
            transformed_mean: 40.0,
            transformed_stddev: 10.0,
        };
        let config = HotnessConfig {
            z_threshold: 2.0,
            z_cool_threshold: 1.0,
            min_observations: 5,
            min_streams: 1,
        };
        // anscombe(v) needs to be < 40 + 1.0*10 = 50 → v < 2500-0.375
        // Use 2000 viewers → anscombe(2000) ≈ 44.72, z ≈ 0.47
        let info = compute_hotness("123", 2000, &stats, &config, true).unwrap();
        assert!(
            !info.is_hot,
            "z={:.2} should cool off (below cool threshold)",
            info.z_score
        );
    }

    // === is_within_hotness_window ===

    #[test]
    fn within_window_when_age_below_max() {
        assert!(is_within_hotness_window(60, 90));
    }

    #[test]
    fn within_window_at_exact_boundary() {
        assert!(is_within_hotness_window(90, 90));
    }

    #[test]
    fn outside_window_when_age_exceeds_max() {
        assert!(!is_within_hotness_window(91, 90));
    }

    #[test]
    fn zero_max_means_infinite_window() {
        assert!(is_within_hotness_window(999_999, 0));
    }

    #[test]
    fn zero_age_always_within_window() {
        assert!(is_within_hotness_window(0, 90));
        assert!(is_within_hotness_window(0, 0));
    }
}
