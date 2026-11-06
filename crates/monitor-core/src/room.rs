//! ログのイベントを順に適用して、現在のルームの状態と入退室の記録を作る。

use crate::location::Location;
use crate::parser::{LogEvent, TimedEvent};
use chrono::NaiveDateTime;
use serde::Serialize;
use std::collections::HashSet;

/// 自分の初期化行が出ないまま、この秒数が過ぎたら以降の入室は「後から来た人」とみなす
const LOCAL_READY_FALLBACK_SECONDS: i64 = 30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Player {
    pub name: String,
    pub user_id: Option<String>,
    pub joined_at: NaiveDateTime,
    /// 自分が入室した時点で既にいた人
    pub initial: bool,
    pub is_local: bool,
}

impl Player {
    pub fn key(&self) -> &str {
        self.user_id.as_deref().unwrap_or(&self.name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FeedKind {
    Join,
    Leave,
    Room,
    Notice,
    Info,
}

/// 入退室の記録の 1 行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedItem {
    pub time: NaiveDateTime,
    pub kind: FeedKind,
    pub text: String,
    pub name: String,
    /// 通知の対象になり得るか（自分自身や先客は対象外）
    pub notifiable: bool,
}

impl FeedItem {
    fn info(time: NaiveDateTime, text: impl Into<String>) -> Self {
        Self {
            time,
            kind: FeedKind::Info,
            text: text.into(),
            name: String::new(),
            notifiable: false,
        }
    }
}

#[derive(Debug, Default)]
pub struct RoomState {
    pub world_name: String,
    pub location: Option<Location>,
    pub entered_at: Option<NaiveDateTime>,
    pub in_room: bool,
    pub running: bool,
    pub local_name: String,
    local_ready: bool,
    players: Vec<Player>,
    /// 記録済みの通知の ID
    seen_notifications: HashSet<String>,
}

impl RoomState {
    /// VRChat の再起動（新しいログファイル）に合わせて初期化する。自分の名前と記録済みの通知は引き継ぐ。
    pub fn reset_session(&mut self) {
        *self = Self {
            local_name: std::mem::take(&mut self.local_name),
            seen_notifications: std::mem::take(&mut self.seen_notifications),
            running: true,
            ..Self::default()
        };
    }

    /// 後から来た人を上に、自分を一番下に並べた一覧。
    pub fn sorted_players(&self) -> Vec<&Player> {
        let mut players: Vec<&Player> = self.players.iter().collect();
        players.sort_by_key(|p| (p.is_local, std::cmp::Reverse(p.joined_at)));
        players
    }

    pub fn player_count(&self) -> usize {
        self.players.len()
    }

    pub fn apply(&mut self, timed: &TimedEvent) -> Vec<FeedItem> {
        let time = timed.time;
        self.running = !matches!(timed.event, LogEvent::AppQuit);
        match &timed.event {
            LogEvent::EnteringRoom { world } => self.on_entering_room(time, world),
            LogEvent::Joining { location } => {
                if self.in_room && self.location.is_none() {
                    self.location = Some(Location::parse(location));
                }
                vec![]
            }
            LogEvent::LeftRoom => {
                // 直後に元のルーム全員分の OnPlayerLeft が出るが、退室として記録しない
                self.in_room = false;
                self.players.clear();
                vec![]
            }
            LogEvent::PlayerJoined { name, user_id } => self.on_player_joined(time, name, user_id),
            LogEvent::PlayerLeft { name, user_id } => self.on_player_left(time, name, user_id),
            LogEvent::LocalPlayer { name } => self.on_local_player(time, name),
            LogEvent::Authenticated { name } => {
                self.local_name.clone_from(name);
                vec![]
            }
            LogEvent::Notification {
                id,
                kind,
                sender,
                world_name,
                message,
            } => {
                // 未読の通知は同じ ID で何度もログに出るので、最初の 1 回だけ記録する
                if !id.is_empty() && !self.seen_notifications.insert(id.clone()) {
                    return vec![];
                }
                vec![notification_item(
                    time,
                    kind,
                    sender,
                    world_name.as_deref(),
                    message.as_deref(),
                )]
            }
            LogEvent::AppQuit => {
                self.in_room = false;
                self.players.clear();
                vec![FeedItem::info(time, "VRChat が終了しました")]
            }
        }
    }

    fn on_entering_room(&mut self, time: NaiveDateTime, world: &str) -> Vec<FeedItem> {
        self.world_name = world.to_string();
        self.location = None;
        self.entered_at = Some(time);
        self.in_room = true;
        self.local_ready = false;
        self.players.clear();
        vec![FeedItem {
            time,
            kind: FeedKind::Room,
            text: world.to_string(),
            name: String::new(),
            notifiable: false,
        }]
    }

    fn on_player_joined(
        &mut self,
        time: NaiveDateTime,
        name: &str,
        user_id: &Option<String>,
    ) -> Vec<FeedItem> {
        if !self.in_room {
            return vec![];
        }
        let waited_too_long = self
            .entered_at
            .is_some_and(|entered| (time - entered).num_seconds() > LOCAL_READY_FALLBACK_SECONDS);
        if waited_too_long {
            self.local_ready = true;
        }
        let player = Player {
            name: name.to_string(),
            user_id: user_id.clone(),
            joined_at: time,
            initial: !self.local_ready,
            is_local: !self.local_name.is_empty() && name == self.local_name,
        };
        let (initial, is_local) = (player.initial, player.is_local);
        self.remove_player(player.user_id.as_deref(), name);
        self.players.push(player);
        if initial {
            return vec![];
        }
        vec![FeedItem {
            time,
            kind: FeedKind::Join,
            text: format!("{name} が入室"),
            name: name.to_string(),
            notifiable: !is_local,
        }]
    }

    fn on_local_player(&mut self, time: NaiveDateTime, name: &str) -> Vec<FeedItem> {
        self.local_name = name.to_string();
        for player in &mut self.players {
            player.is_local = player.name == name;
        }
        if !self.in_room || self.local_ready {
            return vec![];
        }
        self.local_ready = true;
        let others = self.players.iter().filter(|p| !p.is_local).count();
        vec![FeedItem::info(
            time,
            format!("入室完了（先客 {others} 人）"),
        )]
    }

    fn on_player_left(
        &mut self,
        time: NaiveDateTime,
        name: &str,
        user_id: &Option<String>,
    ) -> Vec<FeedItem> {
        if !self.in_room {
            return vec![];
        }
        let player = self.remove_player(user_id.as_deref(), name);
        if player.as_ref().is_some_and(|p| p.is_local) {
            return vec![];
        }
        let stay = player
            .filter(|p| !p.initial)
            .map(|p| {
                let seconds = (time - p.joined_at).num_seconds();
                format!("（滞在 {}）", format_duration(seconds))
            })
            .unwrap_or_default();
        vec![FeedItem {
            time,
            kind: FeedKind::Leave,
            text: format!("{name} が退室{stay}"),
            name: name.to_string(),
            notifiable: true,
        }]
    }

    /// ID で探し、見つからなければ名前で探して取り除く。
    fn remove_player(&mut self, user_id: Option<&str>, name: &str) -> Option<Player> {
        let index = user_id
            .and_then(|id| self.players.iter().position(|p| p.key() == id))
            .or_else(|| self.players.iter().position(|p| p.name == name))?;
        Some(self.players.remove(index))
    }
}

fn notification_item(
    time: NaiveDateTime,
    kind: &str,
    sender: &str,
    world_name: Option<&str>,
    message: Option<&str>,
) -> FeedItem {
    let label = match kind {
        "invite" => "招待".to_string(),
        "requestInvite" => "招待リクエスト".to_string(),
        "friendRequest" => "フレンド申請".to_string(),
        "inviteResponse" => "招待への返信".to_string(),
        "requestInviteResponse" => "招待リクエストへの返信".to_string(),
        "group" => "グループ通知".to_string(),
        other => format!("通知（{other}）"),
    };
    let from = match world_name {
        Some(world) => format!("{sender} → {world}"),
        None => sender.to_string(),
    };
    let detail = [from.as_str(), message.unwrap_or_default()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let text = if detail.is_empty() {
        label
    } else {
        format!("{label}: {detail}")
    };
    FeedItem {
        time,
        kind: FeedKind::Notice,
        text,
        name: sender.to_string(),
        notifiable: true,
    }
}

pub fn format_duration(seconds: i64) -> String {
    let seconds = seconds.max(0);
    let (hours, minutes, seconds) = (seconds / 3600, seconds % 3600 / 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_line;

    const UID_A: &str = "usr_11111111-1111-1111-1111-111111111111";
    const UID_B: &str = "usr_22222222-2222-2222-2222-222222222222";
    const UID_ME: &str = "usr_33333333-3333-3333-3333-333333333333";

    fn line(clock: &str, message: &str) -> String {
        format!("2026.11.03 {clock} Debug      -  {message}")
    }

    fn joined(clock: &str, name: &str, id: &str) -> String {
        line(clock, &format!("[Behaviour] OnPlayerJoined {name} ({id})"))
    }

    fn left(clock: &str, name: &str, id: &str) -> String {
        line(clock, &format!("[Behaviour] OnPlayerLeft {name} ({id})"))
    }

    fn local(clock: &str, name: &str) -> String {
        line(
            clock,
            &format!("[Behaviour] Initialized PlayerAPI \"{name}\" is local"),
        )
    }

    fn entering(clock: &str, world: &str) -> Vec<String> {
        vec![
            line(clock, &format!("[Behaviour] Entering Room: {world}")),
            line(
                clock,
                &format!("[Behaviour] Joining wrld_1:10129~hidden({UID_ME})~region(jp)"),
            ),
        ]
    }

    fn run(state: &mut RoomState, lines: &[String]) -> Vec<FeedItem> {
        lines
            .iter()
            .filter_map(|text| parse_line(text))
            .flat_map(|timed| state.apply(&timed))
            .collect()
    }

    fn player<'a>(state: &'a RoomState, key: &str) -> &'a Player {
        state.players.iter().find(|p| p.key() == key).unwrap()
    }

    #[test]
    fn initial_players_are_not_notified_but_later_joins_are() {
        let mut state = RoomState::default();
        let mut lines = entering("21:59:07", "テストワールド");
        lines.extend([
            joined("21:59:27", "Alice", UID_A),
            joined("21:59:27", "自分", UID_ME),
            local("21:59:27", "自分"),
            line(
                "21:59:27",
                "[Behaviour] Initialized PlayerAPI \"Alice\" is remote",
            ),
            line("22:00:56", "[Behaviour] OnPlayerEnteredRoom"),
            joined("22:01:03", "Bob", UID_B),
        ]);
        let items = run(&mut state, &lines);
        let summary: Vec<_> = items
            .iter()
            .map(|i| (i.kind, i.text.as_str(), i.notifiable))
            .collect();
        assert_eq!(
            summary,
            [
                (FeedKind::Room, "テストワールド", false),
                (FeedKind::Info, "入室完了（先客 1 人）", false),
                (FeedKind::Join, "Bob が入室", true),
            ]
        );
        assert_eq!(state.location.as_ref().unwrap().access, "Friends+");
        assert_eq!(state.player_count(), 3);
        assert!(player(&state, UID_ME).is_local);
        assert!(player(&state, UID_A).initial);
        assert!(!player(&state, UID_B).initial);
    }

    #[test]
    fn leave_shows_stay_time_only_for_players_seen_joining() {
        let mut state = RoomState::default();
        let mut lines = entering("21:59:07", "World");
        lines.extend([
            joined("21:59:27", "Alice", UID_A),
            local("21:59:27", "自分"),
            joined("22:01:03", "Bob", UID_B),
            left("22:05:44", "Bob", UID_B),
            left("22:06:00", "Alice", UID_A),
        ]);
        let leaves: Vec<_> = run(&mut state, &lines)
            .into_iter()
            .filter(|i| i.kind == FeedKind::Leave)
            .map(|i| i.text)
            .collect();
        assert_eq!(leaves, ["Bob が退室（滞在 4:41）", "Alice が退室"]);
        assert_eq!(state.player_count(), 0);
    }

    #[test]
    fn leaving_room_clears_players_without_leave_entries() {
        let mut state = RoomState::default();
        let mut first = entering("21:58:22", "World");
        first.extend([
            joined("21:58:38", "自分", UID_ME),
            local("21:58:38", "自分"),
            joined("21:58:50", "Alice", UID_A),
        ]);
        run(&mut state, &first);

        let mut second = vec![
            line("21:59:06", "[Behaviour] OnLeftRoom"),
            left("21:59:06", "自分", UID_ME),
            left("21:59:06", "Alice", UID_A),
        ];
        second.extend(entering("21:59:07", "次のワールド"));
        second.push(joined("21:59:20", "自分", UID_ME));
        let items = run(&mut state, &second);

        assert_eq!(
            items.iter().map(|i| i.kind).collect::<Vec<_>>(),
            [FeedKind::Room]
        );
        assert_eq!(state.world_name, "次のワールド");
        // 前のルームで自分の名前を覚えているので、初期化行より前でも自分と分かる
        assert!(player(&state, UID_ME).is_local);
        assert!(player(&state, UID_ME).initial);
    }

    #[test]
    fn joins_after_fallback_window_count_as_new_without_local_line() {
        let mut state = RoomState::default();
        let mut lines = entering("21:00:00", "World");
        lines.extend([
            joined("21:00:10", "Alice", UID_A),
            joined("21:01:00", "Bob", UID_B),
        ]);
        let joins: Vec<_> = run(&mut state, &lines)
            .into_iter()
            .filter(|i| i.kind == FeedKind::Join)
            .map(|i| i.name)
            .collect();
        assert_eq!(joins, ["Bob"]);
    }

    #[test]
    fn rejoining_replaces_the_same_player() {
        let mut state = RoomState::default();
        let mut lines = entering("21:00:00", "World");
        lines.extend([
            local("21:00:05", "自分"),
            joined("21:00:10", "Alice", UID_A),
            joined("21:00:20", "Alice", UID_A),
        ]);
        run(&mut state, &lines);
        assert_eq!(state.player_count(), 1);
    }

    #[test]
    fn players_are_sorted_newest_first_with_self_last() {
        let mut state = RoomState::default();
        let mut lines = entering("21:00:00", "World");
        lines.extend([
            joined("21:00:05", "自分", UID_ME),
            local("21:00:05", "自分"),
            joined("21:00:10", "Alice", UID_A),
            joined("21:00:20", "Bob", UID_B),
        ]);
        run(&mut state, &lines);
        let names: Vec<_> = state
            .sorted_players()
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(names, ["Bob", "Alice", "自分"]);
    }

    #[test]
    fn notifications_and_quit_are_recorded() {
        let mut state = RoomState::default();
        let mut lines = entering("22:00:00", "World");
        lines.extend([
            line(
                "22:23:34",
                &format!(
                    "Received Notification: <Notification from username:Alice, sender user \
                     id:{UID_A} to {UID_ME} of type: invite, id: not_1, created at: x, details: \
                     {{{{worldId=wrld_x:1, worldName=Test World}}}}, type:invite, m seen:False, \
                     message: \"\">"
                ),
            ),
            line(
                "23:51:13",
                "VRCApplication: HandleApplicationQuit at 16696.3",
            ),
        ]);
        let items = run(&mut state, &lines);
        assert_eq!(items[1].text, "招待: Alice → Test World");
        assert!(items[1].notifiable);
        assert_eq!(items[2].text, "VRChat が終了しました");
        assert!(!state.running);
        assert!(!state.in_room);
    }

    fn notification(id: &str, kind: &str, sender: &str, message: Option<&str>) -> TimedEvent {
        TimedEvent {
            time: NaiveDateTime::default(),
            event: LogEvent::Notification {
                id: id.into(),
                kind: kind.into(),
                sender: sender.into(),
                world_name: None,
                message: message.map(str::to_string),
            },
        }
    }

    #[test]
    fn group_notice_shows_its_message() {
        let mut state = RoomState::default();
        let items = state.apply(&notification("not_1", "group", "", Some("1 行目\n2 行目")));
        assert_eq!(items[0].kind, FeedKind::Notice);
        assert_eq!(items[0].text, "グループ通知: 1 行目\n2 行目");
        assert!(items[0].notifiable);
    }

    #[test]
    fn notification_text_has_no_dangling_separator() {
        let mut state = RoomState::default();
        let empty = state.apply(&notification("not_1", "group", "", None));
        assert_eq!(empty[0].text, "グループ通知");
        let unknown = state.apply(&notification("not_2", "boop", "Alice", Some("やあ")));
        assert_eq!(unknown[0].text, "通知（boop）: Alice やあ");
    }

    #[test]
    fn repeated_notification_is_recorded_once_even_after_restarting_vrchat() {
        let mut state = RoomState::default();
        assert_eq!(
            state
                .apply(&notification("not_1", "group", "", Some("本文")))
                .len(),
            1
        );
        assert!(
            state
                .apply(&notification("not_1", "group", "", Some("本文")))
                .is_empty()
        );
        state.reset_session();
        assert!(
            state
                .apply(&notification("not_1", "group", "", Some("本文")))
                .is_empty()
        );
        assert_eq!(
            state
                .apply(&notification("not_2", "group", "", Some("別の本文")))
                .len(),
            1
        );
    }

    #[test]
    fn notifications_without_an_id_are_not_merged() {
        let mut state = RoomState::default();
        for _ in 0..2 {
            assert_eq!(
                state
                    .apply(&notification("", "invite", "Alice", None))
                    .len(),
                1
            );
        }
    }

    #[test]
    fn reset_keeps_local_name_and_marks_running() {
        let mut state = RoomState {
            local_name: "自分".into(),
            in_room: true,
            ..RoomState::default()
        };
        state.reset_session();
        assert_eq!(state.local_name, "自分");
        assert!(state.running);
        assert!(!state.in_room);
    }

    #[test]
    fn formats_durations() {
        assert_eq!(format_duration(-5), "0:00");
        assert_eq!(format_duration(281), "4:41");
        assert_eq!(format_duration(3725), "1:02:05");
    }
}
