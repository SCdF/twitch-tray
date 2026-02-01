//! Integration tests for state management

mod common;

use twitch_tray::state::AppState;

#[tokio::test]
async fn state_tracks_newly_live_streams() {
    let state = AppState::new();

    // Initial load
    let stream_a = common::make_stream("a", "StreamerA", 1000);
    state.set_followed_streams(vec![stream_a.clone()]).await;

    // New stream goes live
    let stream_b = common::make_stream("b", "StreamerB", 2000);
    let result = state
        .set_followed_streams(vec![stream_a, stream_b])
        .await;

    assert_eq!(result.newly_live.len(), 1);
    assert_eq!(result.newly_live[0].user_id, "b");
}

#[tokio::test]
async fn state_tracks_streams_going_offline() {
    let state = AppState::new();

    // Both streams live
    let stream_a = common::make_stream("a", "StreamerA", 1000);
    let stream_b = common::make_stream("b", "StreamerB", 2000);
    state
        .set_followed_streams(vec![stream_a.clone(), stream_b.clone()])
        .await;

    // Stream B goes offline
    let result = state.set_followed_streams(vec![stream_a]).await;

    assert!(result.newly_live.is_empty());
    assert_eq!(result.went_offline.len(), 1);
    assert_eq!(result.went_offline[0].user_id, "b");
}

#[tokio::test]
async fn state_authentication_flow() {
    let state = AppState::new();

    // Initially not authenticated
    assert!(!state.is_authenticated().await);

    // Set authenticated
    state
        .set_authenticated(true, "user123".to_string(), "testuser".to_string())
        .await;

    assert!(state.is_authenticated().await);
    assert_eq!(state.get_user_id().await, "user123");
    assert_eq!(state.get_user_login().await, "testuser");

    // Clear state
    state.clear().await;

    assert!(!state.is_authenticated().await);
    assert!(state.get_followed_streams().await.is_empty());
}

#[tokio::test]
async fn state_change_notification() {
    let state = AppState::new();
    let mut rx = state.subscribe();

    // Make a change
    state
        .set_authenticated(true, "user123".to_string(), "test".to_string())
        .await;

    // Should receive notification
    assert!(rx.has_changed().unwrap());
}

#[tokio::test]
async fn scheduled_streams_storage() {
    let state = AppState::new();

    assert!(!state.schedules_loaded().await);

    let scheduled = common::make_many_scheduled(5);
    state.set_scheduled_streams(scheduled.clone()).await;

    assert!(state.schedules_loaded().await);
    assert_eq!(state.get_scheduled_streams().await.len(), 5);
}

#[tokio::test]
async fn followed_channel_ids_storage() {
    let state = AppState::new();

    let ids = vec!["1".to_string(), "2".to_string(), "3".to_string()];
    state.set_followed_channel_ids(ids.clone()).await;

    let retrieved = state.get_followed_channel_ids().await;
    assert_eq!(retrieved, ids);
}
