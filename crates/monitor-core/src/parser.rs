//! VRChat のログ 1 行を、アプリが扱うイベントに変換する。
//!
//! ログの例: `2026.11.03 21:58:22 Debug      -  [Behaviour] Entering Room: World`

use chrono::NaiveDateTime;

const TIME_LENGTH: usize = 19;
const TIME_FORMAT: &str = "%Y.%m.%d %H:%M:%S";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogEvent {
    EnteringRoom {
        world: String,
    },
    Joining {
        location: String,
    },
    LeftRoom,
    PlayerJoined {
        name: String,
        user_id: Option<String>,
    },
    PlayerLeft {
        name: String,
        user_id: Option<String>,
    },
    /// 自分のプレイヤーの初期化。この行より前の入室は、入室時点で既にいた人のもの
    LocalPlayer {
        name: String,
    },
    Authenticated {
        name: String,
    },
    Notification {
        kind: String,
        sender: String,
        world_name: Option<String>,
    },
    AppQuit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimedEvent {
    pub time: NaiveDateTime,
    pub event: LogEvent,
}

pub fn parse_line(line: &str) -> Option<TimedEvent> {
    let line = line.trim_end_matches(['\r', '\n']);
    let stamp = line.get(..TIME_LENGTH)?;
    let rest = line.get(TIME_LENGTH..)?.strip_prefix(' ')?;
    let (_level, message) = rest.split_once(" - ")?;
    let event = parse_message(message.trim_start())?;
    // 大半の行は上で捨てられるので、時刻の解析は対象の行だけに行う
    let time = NaiveDateTime::parse_from_str(stamp, TIME_FORMAT).ok()?;
    Some(TimedEvent { time, event })
}

fn parse_message(message: &str) -> Option<LogEvent> {
    if let Some(body) = message.strip_prefix("[Behaviour] ") {
        return parse_behaviour(body);
    }
    if let Some(body) = message.strip_prefix("Received Notification: ") {
        return parse_notification(body);
    }
    if let Some(body) = message.strip_prefix("VRCApplication: ") {
        let quit =
            body.starts_with("HandleApplicationQuit") || body.starts_with("OnApplicationQuit");
        return quit.then_some(LogEvent::AppQuit);
    }
    let (_, body) = message.split_once("User Authenticated: ")?;
    let (name, user_id) = split_user(body);
    user_id.map(|_| LogEvent::Authenticated { name })
}

fn parse_behaviour(body: &str) -> Option<LogEvent> {
    if let Some(world) = body.strip_prefix("Entering Room: ") {
        return Some(LogEvent::EnteringRoom {
            world: world.to_string(),
        });
    }
    if let Some(rest) = body.strip_prefix("Joining wrld_") {
        let tail = rest.split_whitespace().next().unwrap_or_default();
        return Some(LogEvent::Joining {
            location: format!("wrld_{tail}"),
        });
    }
    if body == "OnLeftRoom" {
        return Some(LogEvent::LeftRoom);
    }
    if let Some(rest) = body.strip_prefix("OnPlayerJoined ") {
        let (name, user_id) = split_user(rest);
        return Some(LogEvent::PlayerJoined { name, user_id });
    }
    if let Some(rest) = body.strip_prefix("OnPlayerLeft ") {
        let (name, user_id) = split_user(rest);
        return Some(LogEvent::PlayerLeft { name, user_id });
    }
    let name = body
        .strip_prefix("Initialized PlayerAPI \"")?
        .strip_suffix("\" is local")?;
    Some(LogEvent::LocalPlayer {
        name: name.to_string(),
    })
}

/// `表示名 (usr_xxx)` を分解する。表示名に括弧が含まれていても、末尾の ID だけを切り離す。
fn split_user(text: &str) -> (String, Option<String>) {
    if let Some((name, tail)) = text.rsplit_once(" (usr_")
        && let Some(id) = tail.strip_suffix(')')
        && !id.is_empty()
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return (name.to_string(), Some(format!("usr_{id}")));
    }
    (text.to_string(), None)
}

fn parse_notification(body: &str) -> Option<LogEvent> {
    let rest = body.strip_prefix("<Notification from username:")?;
    let (sender, rest) = rest.split_once(", sender user id:")?;
    let (_, rest) = rest.split_once(" of type: ")?;
    let (kind, rest) = rest.split_once(',')?;
    let world_name = rest
        .split_once("details: {{")
        .and_then(|(_, details)| details.split_once("}}"))
        .and_then(|(details, _)| details.split_once("worldName="))
        .and_then(|(_, tail)| tail.split(',').next())
        .filter(|name| !name.is_empty())
        .map(str::to_string);
    Some(LogEvent::Notification {
        kind: kind.to_string(),
        sender: sender.to_string(),
        world_name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const UID: &str = "usr_11111111-1111-1111-1111-111111111111";

    fn line(message: &str) -> String {
        format!("2026.11.03 21:59:27 Debug      -  {message}")
    }

    fn event(message: &str) -> Option<LogEvent> {
        parse_line(&line(message)).map(|timed| timed.event)
    }

    #[test]
    fn reads_time() {
        let timed = parse_line(&line("[Behaviour] OnLeftRoom")).unwrap();
        assert_eq!(
            timed.time.format("%Y-%m-%d %H:%M:%S").to_string(),
            "2026-11-03 21:59:27"
        );
    }

    #[test]
    fn splits_user_id_from_names_with_brackets() {
        assert_eq!(
            event(&format!("[Behaviour] OnPlayerJoined A (B) C ({UID})")),
            Some(LogEvent::PlayerJoined {
                name: "A (B) C".into(),
                user_id: Some(UID.into())
            })
        );
    }

    #[test]
    fn accepts_old_format_without_user_id() {
        assert_eq!(
            event("[Behaviour] OnPlayerLeft 古い形式"),
            Some(LogEvent::PlayerLeft {
                name: "古い形式".into(),
                user_id: None
            })
        );
    }

    #[test]
    fn similar_lines_are_not_misread() {
        for message in [
            "[Behaviour] OnPlayerJoinComplete Alice",
            "[Behaviour] OnPlayerLeftRoom",
            "[Behaviour] OnPlayerEnteredRoom",
            "[Behaviour] Joining or Creating Room: World",
            "[Behaviour] Destination set: wrld_x:1~region(jp)",
            "[Behaviour] Initialized PlayerAPI \"Bob\" is remote",
            "VRCApplication: Something else",
        ] {
            assert_eq!(event(message), None, "{message}");
        }
    }

    #[test]
    fn lines_without_timestamp_are_ignored() {
        assert_eq!(parse_line("[Behaviour] OnPlayerJoined Alice"), None);
        assert_eq!(
            parse_line("not a date here Debug      -  [Behaviour] OnLeftRoom"),
            None
        );
    }

    #[test]
    fn handles_crlf() {
        let text = line("[Behaviour] Initialized PlayerAPI \"自分\" is local") + "\r\n";
        assert_eq!(
            parse_line(&text).map(|timed| timed.event),
            Some(LogEvent::LocalPlayer {
                name: "自分".into()
            })
        );
    }

    #[test]
    fn reads_room_lines() {
        assert_eq!(
            event("[Behaviour] Entering Room: テスト - ワールド"),
            Some(LogEvent::EnteringRoom {
                world: "テスト - ワールド".into()
            })
        );
        assert_eq!(
            event("[Behaviour] Joining wrld_1:2~region(jp)"),
            Some(LogEvent::Joining {
                location: "wrld_1:2~region(jp)".into()
            })
        );
    }

    #[test]
    fn reads_application_lines() {
        assert_eq!(
            event("VRCApplication: HandleApplicationQuit at 16696.3"),
            Some(LogEvent::AppQuit)
        );
        assert_eq!(
            event(&format!("User Authenticated: 自分 ({UID})")),
            Some(LogEvent::Authenticated {
                name: "自分".into()
            })
        );
    }

    #[test]
    fn reads_invite_with_world_name() {
        let message = format!(
            "Received Notification: <Notification from username:Alice, sender user id:{UID} to \
             usr_2 of type: invite, id: not_1, created at: 11/03/2026 13:23:28 UTC, details: \
             {{{{worldId=wrld_x:1~private(usr_y), worldName=Test World}}}}, type:invite, m \
             seen:False, message: \"\">"
        );
        assert_eq!(
            event(&message),
            Some(LogEvent::Notification {
                kind: "invite".into(),
                sender: "Alice".into(),
                world_name: Some("Test World".into()),
            })
        );
    }

    #[test]
    fn reads_request_invite_without_world_name() {
        let message = format!(
            "Received Notification: <Notification from username:Bob, sender user id:{UID} to \
             usr_2 of type: requestInvite, id: not_2, created at: x, details: {{{{}}}}, \
             type:requestInvite, m seen:False, message: \"\">"
        );
        assert_eq!(
            event(&message),
            Some(LogEvent::Notification {
                kind: "requestInvite".into(),
                sender: "Bob".into(),
                world_name: None,
            })
        );
    }
}
