//! 監視スレッドからのメッセージを受けて状態を更新し、画面に渡す形に変換する。

use crate::location::Location;
use crate::room::{FeedItem, FeedKind, RoomState};
use crate::settings::Settings;
use crate::sound::Sound;
use crate::watcher::WatchMessage;
use chrono::{Local, NaiveDateTime, TimeZone};
use serde::Serialize;
use std::collections::VecDeque;

/// 入退室の記録を保持する上限（超えたら古いものから捨てる）
pub const FEED_LIMIT: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Loading,
    NoLog,
    Error,
    Offline,
    InRoom,
    Moving,
}

#[derive(Debug)]
pub struct Monitor {
    room: RoomState,
    /// 新しいものが先頭
    feed: VecDeque<FeedItem>,
    replaying: bool,
    no_log: bool,
    problem: Option<String>,
}

impl Default for Monitor {
    fn default() -> Self {
        Self {
            room: RoomState::default(),
            feed: VecDeque::new(),
            replaying: true,
            no_log: false,
            problem: None,
        }
    }
}

impl Monitor {
    /// メッセージを反映し、通知の候補を返す（起動前のログからは通知しない）。
    pub fn handle(&mut self, message: WatchMessage) -> Vec<FeedItem> {
        match message {
            WatchMessage::File { replay, .. } => {
                self.no_log = false;
                self.problem = None;
                if !replay {
                    self.room.reset_session();
                    self.push(FeedItem {
                        time: Local::now().naive_local(),
                        kind: FeedKind::Info,
                        text: "新しいログを検出（VRChat を起動）".into(),
                        name: String::new(),
                        notifiable: false,
                    });
                }
                vec![]
            }
            WatchMessage::Events { events, replay } => {
                self.problem = None;
                let mut alerts = Vec::new();
                for timed in &events {
                    for item in self.room.apply(timed) {
                        if !replay && item.notifiable {
                            alerts.push(item.clone());
                        }
                        self.push(item);
                    }
                }
                alerts
            }
            WatchMessage::ReplayDone => {
                self.replaying = false;
                vec![]
            }
            WatchMessage::NoLog { dir } => {
                self.replaying = false;
                self.no_log = true;
                self.problem = Some(format!("ログが見つかりません: {}", dir.display()));
                vec![]
            }
            WatchMessage::Error { message } => {
                self.problem = Some(format!("読み込みエラー: {message}"));
                vec![]
            }
        }
    }

    fn push(&mut self, item: FeedItem) {
        self.feed.push_front(item);
        self.feed.truncate(FEED_LIMIT);
    }

    pub fn status(&self) -> Status {
        if self.replaying {
            Status::Loading
        } else if self.no_log {
            Status::NoLog
        } else if self.problem.is_some() {
            Status::Error
        } else if !self.room.running {
            Status::Offline
        } else if self.room.in_room {
            Status::InRoom
        } else {
            Status::Moving
        }
    }

    /// ウィンドウの題名とトレイの説明に使う要約。
    pub fn summary(&self) -> Option<String> {
        self.room
            .in_room
            .then(|| format!("{}人 · {}", self.room.player_count(), self.room.world_name))
    }

    pub fn world_name(&self) -> &str {
        &self.room.world_name
    }

    pub fn view(&self) -> RoomView {
        let room = &self.room;
        let in_room = room.in_room;
        RoomView {
            status: self.status(),
            problem: self.problem.clone(),
            world_name: in_room.then(|| room.world_name.clone()),
            location: if in_room { room.location.clone() } else { None },
            entered_at: if in_room {
                room.entered_at.map(to_millis)
            } else {
                None
            },
            players: room
                .sorted_players()
                .into_iter()
                .map(|p| PlayerView {
                    key: p.key().to_string(),
                    name: p.name.clone(),
                    joined_at: to_millis(p.joined_at),
                    initial: p.initial,
                    is_local: p.is_local,
                })
                .collect(),
            feed: self
                .feed
                .iter()
                .map(|item| FeedView {
                    time: to_millis(item.time),
                    kind: item.kind,
                    text: item.text.clone(),
                })
                .collect(),
        }
    }
}

/// ログの時刻は PC の現地時刻なので、画面に渡すために UNIX 時刻（ミリ秒）へ変換する。
fn to_millis(time: NaiveDateTime) -> i64 {
    Local.from_local_datetime(&time).earliest().map_or_else(
        || time.and_utc().timestamp_millis(),
        |t| t.timestamp_millis(),
    )
}

/// 退室は設定でオンのときだけ通知する。
pub fn wants_alert(item: &FeedItem, notify_leave: bool) -> bool {
    match item.kind {
        FeedKind::Join | FeedKind::Notice => true,
        FeedKind::Leave => notify_leave,
        FeedKind::Room | FeedKind::Info => false,
    }
}

/// 同時に鳴らせる音は 1 つなので、入室、招待などの通知、退室の順に優先する。無音に設定した種類は飛ばす。
pub fn alert_sound(items: &[&FeedItem], settings: &Settings) -> Option<Sound> {
    [
        (FeedKind::Join, settings.sound_join),
        (FeedKind::Notice, settings.sound_notice),
        (FeedKind::Leave, settings.sound_leave),
    ]
    .into_iter()
    .find(|&(kind, sound)| sound != Sound::Silent && items.iter().any(|item| item.kind == kind))
    .map(|(_, sound)| sound)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomView {
    pub status: Status,
    pub problem: Option<String>,
    pub world_name: Option<String>,
    pub location: Option<Location>,
    pub entered_at: Option<i64>,
    pub players: Vec<PlayerView>,
    pub feed: Vec<FeedView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerView {
    pub key: String,
    pub name: String,
    pub joined_at: i64,
    pub initial: bool,
    pub is_local: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedView {
    pub time: i64,
    pub kind: FeedKind,
    pub text: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_line;
    use std::path::PathBuf;

    fn events(lines: &[&str], replay: bool) -> WatchMessage {
        WatchMessage::Events {
            events: lines.iter().filter_map(|line| parse_line(line)).collect(),
            replay,
        }
    }

    const ROOM: &str = "2026.11.03 21:00:00 Debug      -  [Behaviour] Entering Room: World";
    const LOCAL: &str =
        "2026.11.03 21:00:05 Debug      -  [Behaviour] Initialized PlayerAPI \"Me\" is local";
    const BOB: &str = "2026.11.03 21:01:00 Debug      -  [Behaviour] OnPlayerJoined Bob (usr_1)";
    const BOB_LEFT: &str = "2026.11.03 21:02:00 Debug      -  [Behaviour] OnPlayerLeft Bob (usr_1)";

    #[test]
    fn replay_does_not_alert_but_is_recorded() {
        let mut monitor = Monitor::default();
        assert_eq!(monitor.status(), Status::Loading);
        let alerts = monitor.handle(events(&[ROOM, LOCAL, BOB], true));
        assert!(alerts.is_empty());
        monitor.handle(WatchMessage::ReplayDone);
        assert_eq!(monitor.status(), Status::InRoom);
        assert_eq!(monitor.summary().as_deref(), Some("1人 · World"));
        assert_eq!(monitor.view().feed[0].text, "Bob が入室");
    }

    #[test]
    fn live_join_alerts() {
        let mut monitor = Monitor::default();
        monitor.handle(events(&[ROOM, LOCAL], true));
        monitor.handle(WatchMessage::ReplayDone);
        let alerts = monitor.handle(events(&[BOB], false));
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].name, "Bob");
    }

    #[test]
    fn leave_alert_follows_setting() {
        let mut monitor = Monitor::default();
        monitor.handle(events(&[ROOM, LOCAL, BOB], true));
        monitor.handle(WatchMessage::ReplayDone);
        let alerts = monitor.handle(events(&[BOB_LEFT], false));
        assert!(wants_alert(&alerts[0], true));
        assert!(!wants_alert(&alerts[0], false));
    }

    fn item(kind: FeedKind) -> FeedItem {
        FeedItem {
            time: NaiveDateTime::default(),
            kind,
            text: String::new(),
            name: String::new(),
            notifiable: true,
        }
    }

    #[test]
    fn each_kind_uses_its_own_sound() {
        let settings = Settings::default();
        for (kind, sound) in [
            (FeedKind::Join, settings.sound_join),
            (FeedKind::Leave, settings.sound_leave),
            (FeedKind::Notice, settings.sound_notice),
        ] {
            assert_eq!(alert_sound(&[&item(kind)], &settings), Some(sound));
        }
    }

    #[test]
    fn join_sound_wins_when_several_kinds_arrive_together() {
        let settings = Settings::default();
        let (leave, join, notice) = (
            item(FeedKind::Leave),
            item(FeedKind::Join),
            item(FeedKind::Notice),
        );
        assert_eq!(
            alert_sound(&[&leave, &notice, &join], &settings),
            Some(settings.sound_join)
        );
        assert_eq!(
            alert_sound(&[&leave, &notice], &settings),
            Some(settings.sound_notice)
        );
    }

    #[test]
    fn silent_kinds_are_skipped() {
        let settings = Settings {
            sound_join: Sound::Silent,
            ..Settings::default()
        };
        let (join, leave) = (item(FeedKind::Join), item(FeedKind::Leave));
        assert_eq!(alert_sound(&[&join], &settings), None);
        assert_eq!(
            alert_sound(&[&join, &leave], &settings),
            Some(settings.sound_leave)
        );
    }

    #[test]
    fn new_log_file_resets_room_and_counts_as_running() {
        let mut monitor = Monitor::default();
        monitor.handle(events(&[ROOM], true));
        monitor.handle(WatchMessage::ReplayDone);
        monitor.handle(WatchMessage::File {
            path: PathBuf::from("new.txt"),
            replay: false,
        });
        assert_eq!(monitor.status(), Status::Moving);
        assert_eq!(monitor.summary(), None);
    }

    #[test]
    fn missing_log_and_errors_are_reported() {
        let mut monitor = Monitor::default();
        monitor.handle(WatchMessage::NoLog {
            dir: PathBuf::from("X"),
        });
        assert_eq!(monitor.status(), Status::NoLog);
        monitor.handle(WatchMessage::File {
            path: PathBuf::from("a.txt"),
            replay: false,
        });
        monitor.handle(WatchMessage::Error {
            message: "denied".into(),
        });
        assert_eq!(monitor.status(), Status::Error);
        assert_eq!(
            monitor.view().problem.as_deref(),
            Some("読み込みエラー: denied")
        );
        monitor.handle(events(&[ROOM], false));
        assert_eq!(monitor.status(), Status::InRoom);
    }

    #[test]
    fn feed_is_limited_and_newest_first() {
        let mut monitor = Monitor::default();
        monitor.handle(events(&[ROOM, LOCAL], true));
        for i in 0..(FEED_LIMIT + 10) {
            let line = format!(
                "2026.11.03 22:00:00 Debug      -  [Behaviour] OnPlayerJoined P{i} (usr_{i})"
            );
            monitor.handle(events(&[&line], true));
        }
        let view = monitor.view();
        assert_eq!(view.feed.len(), FEED_LIMIT);
        assert_eq!(view.feed[0].text, format!("P{} が入室", FEED_LIMIT + 9));
    }

    #[test]
    fn view_serializes_for_the_ui() {
        let mut monitor = Monitor::default();
        monitor.handle(events(&[ROOM, LOCAL], true));
        monitor.handle(WatchMessage::ReplayDone);
        let json = serde_json::to_value(monitor.view()).unwrap();
        assert_eq!(json["status"], "inRoom");
        assert_eq!(json["worldName"], "World");
        assert_eq!(json["feed"][0]["kind"], "info");
    }
}
