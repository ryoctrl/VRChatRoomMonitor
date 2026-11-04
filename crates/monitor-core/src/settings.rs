//! 利用者の設定。ファイルが無い、または壊れている場合は既定値で動く。

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
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
