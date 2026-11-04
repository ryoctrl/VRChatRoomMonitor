//! SteamVR に渡す登録ファイル（`.vrmanifest`）。exe の場所が変わったら書き直す。

use serde_json::json;
use std::io;
use std::path::{Path, PathBuf};

pub const APP_KEY: &str = "ryoctrl.vrchatroommonitor";
const FILE_NAME: &str = "VRChatRoomMonitor.vrmanifest";

fn content(exe: &Path, image: &Path, launch_argument: &str) -> String {
    let manifest = json!({
        "source": "builtin",
        "applications": [{
            "app_key": APP_KEY,
            "launch_type": "binary",
            "binary_path_windows": exe,
            "arguments": launch_argument,
            // この指定があるアプリだけが「起動時のオーバーレイアプリ」の一覧に載る
            "is_dashboard_overlay": true,
            "image_path": image,
            "strings": { "en_us": {
                "name": "VRChat Room Monitor",
                "description": "Shows the current VRChat room and join log on the desktop.",
            }},
        }],
    });
    serde_json::to_string_pretty(&manifest).expect("manifest is valid JSON")
}

/// 登録ファイルを書いてパスを返す。内容が同じなら書き換えない。
pub fn write(dir: &Path, exe: &Path, image: &Path, launch_argument: &str) -> io::Result<PathBuf> {
    let path = dir.join(FILE_NAME);
    let text = content(exe, image, launch_argument);
    if std::fs::read_to_string(&path).is_ok_and(|current| current == text) {
        return Ok(path);
    }
    std::fs::create_dir_all(dir)?;
    std::fs::write(&path, text)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(test: &str) -> Self {
            Self(std::env::temp_dir().join(format!("steamvr-{test}-{}", std::process::id())))
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn declares_overlay_launched_with_argument() {
        let dir = TempDir::new("declares");
        let path = write(
            &dir.0,
            Path::new(r"C:\app\a.exe"),
            Path::new(r"C:\app\i.png"),
            "--steamvr",
        )
        .unwrap();
        let json: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let app = &json["applications"][0];
        assert_eq!(app["app_key"], APP_KEY);
        assert_eq!(app["binary_path_windows"], r"C:\app\a.exe");
        assert_eq!(app["arguments"], "--steamvr");
        assert_eq!(app["is_dashboard_overlay"], true);
        assert_eq!(app["image_path"], r"C:\app\i.png");
    }

    #[test]
    fn follows_exe_location() {
        let dir = TempDir::new("follows");
        let image = Path::new(r"C:\app\i.png");
        let path = write(&dir.0, Path::new(r"C:\app\a.exe"), image, "--steamvr").unwrap();
        write(&dir.0, Path::new(r"D:\moved\a.exe"), image, "--steamvr").unwrap();
        let text = std::fs::read_to_string(path).unwrap();
        assert!(text.contains(r"D:\\moved\\a.exe"));
        assert!(!text.contains(r"C:\\app\\a.exe"));
    }

    #[test]
    fn keeps_non_ascii_paths() {
        let dir = TempDir::new("nonascii");
        let path = write(
            &dir.0,
            Path::new(r"C:\ユーザー\a.exe"),
            Path::new(r"C:\ユーザー\i.png"),
            "--steamvr",
        )
        .unwrap();
        assert!(std::fs::read_to_string(path).unwrap().contains("ユーザー"));
    }
}
