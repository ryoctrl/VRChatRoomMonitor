//! 利用者の設定。ファイルが無い、または壊れている場合は既定値で動く。

use crate::sound::Sound;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub always_on_top: bool,
    pub popup: bool,
    pub sound: bool,
    pub notify_leave: bool,
    /// 閉じてもトレイで動き続けることを案内したか
    pub tray_hint_shown: bool,
    /// 利用者が SteamVR の一覧から削除した（自動で登録し直さない）
    pub steamvr_removed: bool,
    /// 更新を確認する（GitHub に通信する）
    pub check_updates: bool,
    /// 入退室ログを、現在のワールドの分だけに絞る（配信に映す場合など）。
    /// 0.2.1 では別の名前で保存していたので、その設定も読み込む
    #[serde(alias = "currentWorldJoinsOnly")]
    pub current_world_only: bool,
    /// ウィンドウの題名をアプリ名だけにする（ウィンドウキャプチャで題名を一致させる場合など）
    pub fixed_title: bool,
    pub sound_join: Sound,
    pub sound_leave: Sound,
    /// 招待やグループ通知など
    pub sound_notice: Sound,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            always_on_top: false,
            popup: true,
            sound: true,
            notify_leave: false,
            tray_hint_shown: false,
            steamvr_removed: false,
            check_updates: true,
            current_world_only: false,
            fixed_title: false,
            sound_join: Sound::Chime,
            sound_leave: Sound::Soft,
            sound_notice: Sound::Bell,
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_path(test: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!(
                "monitor-core-settings-{test}-{}",
                std::process::id()
            ))
            .join("settings.json")
    }

    #[test]
    fn missing_file_uses_defaults() {
        assert_eq!(Settings::load(&temp_path("missing")), Settings::default());
    }

    #[test]
    fn saves_and_loads() {
        let path = temp_path("roundtrip");
        let settings = Settings {
            always_on_top: true,
            sound: false,
            ..Settings::default()
        };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn broken_file_uses_defaults_and_partial_file_keeps_known_values() {
        let path = temp_path("broken");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        std::fs::write(&path, r#"{"sound": false}"#).unwrap();
        let loaded = Settings::load(&path);
        assert!(!loaded.sound);
        assert!(loaded.popup);
        // 以前の版の設定ファイルには無い項目は、既定値で始まる
        assert!(!loaded.current_world_only);
        assert!(!loaded.fixed_title);
        assert_eq!(loaded.sound_join, Sound::Chime);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn fixed_title_uses_the_name_the_ui_expects() {
        let settings = Settings {
            fixed_title: true,
            ..Settings::default()
        };
        let json = serde_json::to_value(settings).unwrap();
        assert_eq!(json["fixedTitle"], true);
    }

    #[test]
    fn sounds_use_the_names_the_ui_expects_and_are_saved() {
        let path = temp_path("sounds");
        let settings = Settings {
            sound_join: Sound::Marimba,
            sound_leave: Sound::Silent,
            sound_notice: Sound::Windows,
            ..Settings::default()
        };
        let json = serde_json::to_value(&settings).unwrap();
        assert_eq!(json["soundJoin"], "marimba");
        assert_eq!(json["soundLeave"], "silent");
        assert_eq!(json["soundNotice"], "windows");
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn join_and_leave_sound_different_by_default() {
        let settings = Settings::default();
        assert_ne!(settings.sound_join, settings.sound_leave);
        assert_ne!(settings.sound_join, settings.sound_notice);
    }

    #[test]
    fn current_world_filter_uses_the_name_the_ui_expects() {
        let settings = Settings {
            current_world_only: true,
            ..Settings::default()
        };
        let json = serde_json::to_value(settings).unwrap();
        assert_eq!(json["currentWorldOnly"], true);
    }

    #[test]
    fn current_world_filter_keeps_the_setting_saved_by_0_2_1() {
        let path = temp_path("old-filter-name");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"currentWorldJoinsOnly": true}"#).unwrap();
        assert!(Settings::load(&path).current_world_only);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
