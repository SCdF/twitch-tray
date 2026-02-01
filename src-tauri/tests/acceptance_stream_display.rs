//! Acceptance tests for stream display behavior

use chrono::{Duration, Utc};
use twitch_tray::tray::menu_data::MenuData;
use twitch_tray::twitch::{ScheduledStream, Stream};

fn make_stream(user_id: &str, user_name: &str, viewer_count: i64) -> Stream {
    Stream {
        id: format!("stream_{}", user_id),
        user_id: user_id.to_string(),
        user_login: user_name.to_lowercase(),
        user_name: user_name.to_string(),
        game_id: "game123".to_string(),
        game_name: "Test Game".to_string(),
        title: "Test Stream".to_string(),
        viewer_count,
        started_at: Utc::now() - Duration::hours(1),
        thumbnail_url: "https://example.com/thumb.jpg".to_string(),
        tags: vec![],
    }
}

fn make_scheduled(broadcaster_id: &str, broadcaster_name: &str, hours_from_now: i64) -> ScheduledStream {
    ScheduledStream {
        id: format!("sched_{}", broadcaster_id),
        broadcaster_id: broadcaster_id.to_string(),
        broadcaster_name: broadcaster_name.to_string(),
        broadcaster_login: broadcaster_name.to_lowercase(),
        title: "Scheduled Stream".to_string(),
        start_time: Utc::now() + Duration::hours(hours_from_now),
        end_time: None,
        category: Some("Gaming".to_string()),
        category_id: Some("123".to_string()),
        is_recurring: false,
    }
}

#[test]
fn live_streams_are_sorted_by_viewers_highest_first() {
    let streams = vec![
        make_stream("1", "SmallStreamer", 100),
        make_stream("2", "BigStreamer", 5000),
        make_stream("3", "MediumStreamer", 1000),
    ];

    let menu = MenuData::from_state(streams, vec![], true, true);

    assert_eq!(menu.live_streams.len(), 3);
    assert_eq!(menu.live_streams[0].viewer_count, 5000);
    assert_eq!(menu.live_streams[1].viewer_count, 1000);
    assert_eq!(menu.live_streams[2].viewer_count, 100);
}

#[test]
fn more_than_10_streams_shows_overflow() {
    let streams: Vec<Stream> = (0..15)
        .map(|i| make_stream(&i.to_string(), &format!("Streamer{}", i), (15 - i) as i64 * 100))
        .collect();

    let menu = MenuData::from_state(streams, vec![], true, true);

    assert_eq!(menu.live_streams.len(), 10);
    assert_eq!(menu.live_overflow.len(), 5);
    assert!(menu.has_live_overflow());
    assert_eq!(menu.live_count(), 15);
}

#[test]
fn exactly_10_streams_no_overflow() {
    let streams: Vec<Stream> = (0..10)
        .map(|i| make_stream(&i.to_string(), &format!("Streamer{}", i), (10 - i) as i64 * 100))
        .collect();

    let menu = MenuData::from_state(streams, vec![], true, true);

    assert_eq!(menu.live_streams.len(), 10);
    assert!(menu.live_overflow.is_empty());
    assert!(!menu.has_live_overflow());
}

#[test]
fn scheduled_streams_overflow_at_5() {
    let scheduled: Vec<ScheduledStream> = (0..8)
        .map(|i| make_scheduled(&i.to_string(), &format!("Broadcaster{}", i), i as i64 + 1))
        .collect();

    let menu = MenuData::from_state(vec![], scheduled, true, true);

    assert_eq!(menu.scheduled.len(), 5);
    assert_eq!(menu.scheduled_overflow.len(), 3);
    assert!(menu.has_scheduled_overflow());
}

#[test]
fn empty_state_when_no_streams() {
    let menu = MenuData::from_state(vec![], vec![], true, true);

    assert!(menu.live_streams.is_empty());
    assert!(menu.live_overflow.is_empty());
    assert!(menu.scheduled.is_empty());
    assert!(menu.scheduled_overflow.is_empty());
}

#[test]
fn unauthenticated_state() {
    let menu = MenuData::from_state(vec![], vec![], false, false);

    assert!(!menu.authenticated);
    assert!(!menu.schedules_loaded);
}

#[test]
fn authenticated_but_schedules_not_loaded() {
    let menu = MenuData::from_state(vec![], vec![], true, false);

    assert!(menu.authenticated);
    assert!(!menu.schedules_loaded);
}

#[test]
fn stream_entries_have_correct_format() {
    let stream = make_stream("123", "TestStreamer", 1000);
    let menu = MenuData::from_state(vec![stream], vec![], true, true);

    assert_eq!(menu.live_streams.len(), 1);
    assert_eq!(menu.live_streams[0].id, "stream_teststreamer");
    assert_eq!(menu.live_streams[0].user_login, "teststreamer");
    assert_eq!(menu.live_streams[0].viewer_count, 1000);
}

#[test]
fn scheduled_entries_have_correct_format() {
    let scheduled = make_scheduled("456", "TestBroadcaster", 2);
    let menu = MenuData::from_state(vec![], vec![scheduled], true, true);

    assert_eq!(menu.scheduled.len(), 1);
    assert_eq!(menu.scheduled[0].id, "scheduled_testbroadcaster");
    assert_eq!(menu.scheduled[0].broadcaster_login, "testbroadcaster");
}
