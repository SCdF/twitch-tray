use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct PlasmoidState {
    pub authenticated: bool,
    pub login_state: LoginStateDto,
    pub live: LiveSectionDto,
    pub categories: Vec<CategorySectionDto>,
    pub schedule: ScheduleSectionDto,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
#[serde(tag = "type")]
pub enum LoginStateDto {
    Idle,
    PendingCode {
        user_code: String,
        verification_uri: String,
    },
    AwaitingConfirmation,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct LiveSectionDto {
    pub visible: Vec<LiveStreamDto>,
    pub overflow: Vec<LiveStreamDto>,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct LiveStreamDto {
    pub user_login: String,
    pub user_name: String,
    pub game_name: String,
    pub title: String,
    pub profile_image_url: String,
    pub viewer_count_formatted: String,
    pub duration_formatted: String,
    pub is_favourite: bool,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct CategorySectionDto {
    pub id: String,
    pub name: String,
    pub total_viewers_formatted: String,
    pub stream_count_formatted: String,
    pub streams: Vec<CategoryStreamDto>,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct CategoryStreamDto {
    pub user_login: String,
    pub user_name: String,
    pub viewer_count_formatted: String,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct ScheduleSectionDto {
    pub lookahead_hours: u64,
    pub loaded: bool,
    pub visible: Vec<ScheduledStreamDto>,
    pub overflow: Vec<ScheduledStreamDto>,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct ScheduledStreamDto {
    pub broadcaster_login: String,
    pub broadcaster_name: String,
    pub start_time_formatted: String,
    pub title: String,
    pub category: String,
    pub profile_image_url: String,
    pub is_inferred: bool,
    pub is_favourite: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_live() -> LiveSectionDto {
        LiveSectionDto {
            visible: vec![],
            overflow: vec![],
        }
    }

    fn empty_schedule() -> ScheduleSectionDto {
        ScheduleSectionDto {
            lookahead_hours: 24,
            loaded: false,
            visible: vec![],
            overflow: vec![],
        }
    }

    #[test]
    fn unauthenticated_state_round_trips_through_json() {
        let state = PlasmoidState {
            authenticated: false,
            login_state: LoginStateDto::Idle,
            live: empty_live(),
            categories: vec![],
            schedule: empty_schedule(),
        };
        let json = serde_json::to_string(&state).unwrap();
        let parsed: PlasmoidState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, parsed);
    }

    #[test]
    fn live_stream_dto_round_trips_through_json() {
        let dto = LiveStreamDto {
            user_login: "streamer1".to_string(),
            user_name: "Streamer1".to_string(),
            game_name: "Chess".to_string(),
            title: "Playing chess with viewers!".to_string(),
            profile_image_url: "https://example.com/avatar.jpg".to_string(),
            viewer_count_formatted: "12k".to_string(),
            duration_formatted: "1h 30m".to_string(),
            is_favourite: true,
        };
        let json = serde_json::to_string(&dto).unwrap();
        let parsed: LiveStreamDto = serde_json::from_str(&json).unwrap();
        assert_eq!(dto, parsed);
    }

    #[test]
    fn scheduled_stream_dto_round_trips_through_json() {
        let dto = ScheduledStreamDto {
            broadcaster_login: "streamer2".to_string(),
            broadcaster_name: "Streamer2".to_string(),
            start_time_formatted: "Tomorrow 8:00 PM".to_string(),
            title: "Evening Stream".to_string(),
            category: "Just Chatting".to_string(),
            profile_image_url: "https://example.com/avatar.jpg".to_string(),
            is_inferred: true,
            is_favourite: false,
        };
        let json = serde_json::to_string(&dto).unwrap();
        let parsed: ScheduledStreamDto = serde_json::from_str(&json).unwrap();
        assert_eq!(dto, parsed);
    }

    #[test]
    fn login_state_pending_code_round_trips_through_json() {
        let state = LoginStateDto::PendingCode {
            user_code: "ABCD-1234".to_string(),
            verification_uri: "https://twitch.tv/activate".to_string(),
        };
        let json = serde_json::to_string(&state).unwrap();
        let parsed: LoginStateDto = serde_json::from_str(&json).unwrap();
        assert_eq!(state, parsed);
    }
}
