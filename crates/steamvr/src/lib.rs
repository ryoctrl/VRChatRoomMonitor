//! SteamVR（OpenVR）との連携。
//!
//! SteamVR の「起動時のオーバーレイアプリ」の一覧に載るまでの流れは次のとおり。
//!
//! 1. `manifest`: アプリの情報を書いた登録ファイル（`.vrmanifest`）を作る
//! 2. `api`: `AddApplicationManifest` で登録ファイルを SteamVR に渡す。
//!    SteamVR はそのパスを自身の設定（`config/appconfig.json`）に保存し、起動のたびに読み込む
//! 3. `api`: `SetApplicationAutoLaunch` で同時起動を切り替える。
//!    SteamVR の設定画面で切り替えた場合と同じ設定（`config/vrappconfig/`）が変わる
//!
//! `service` はこれらを専用のスレッドで実行し、SteamVR の起動と終了に追従する。

mod api;
pub mod manifest;
mod service;

pub use api::{Error, self_check};
pub use service::{Config, Handle, Status, spawn};

/// 実際の SteamVR を使う確認。SteamVR を起動した状態で
/// `cargo test -p steamvr -- --ignored --nocapture` を実行する。
#[cfg(test)]
mod live {
    use super::api::{ConnectionKind, OpenVr};
    use super::manifest::{self, APP_KEY};
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    #[test]
    #[ignore = "起動中の SteamVR が必要"]
    fn registers_toggles_and_removes() {
        let dir = std::env::temp_dir().join(format!("steamvr-live-{}", std::process::id()));
        let api = Rc::new(OpenVr::load(&dir).expect("load library"));
        assert!(api.is_runtime_installed());
        let session = api.connect(ConnectionKind::Background).expect("connect");

        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            assert!(!session.poll_quit());
            std::thread::sleep(Duration::from_millis(200));
        }
        println!("polled events without a quit request");

        let was_installed = session.is_installed(APP_KEY);
        println!("installed before: {was_installed}");
        assert!(
            !was_installed,
            "the real app is registered; test would disturb it"
        );

        // 存在しない exe を指す登録は SteamVR にアプリとして認識されないので、実在する exe を指す。
        // 自動起動を一瞬オンにするので、万一起動されても何も残らないコマンドを選んでいる
        let path = manifest::write(
            &dir,
            std::path::Path::new(r"C:\Windows\System32\hostname.exe"),
            &dir.join("icon.png"),
            "--steamvr",
        )
        .expect("write manifest");
        let path = path.to_str().unwrap();
        session.add_manifest(path).expect("add manifest");

        // 途中で失敗しても、実際の SteamVR の設定にテストの登録を残さない
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            assert!(session.is_installed(APP_KEY));
            assert!(!session.auto_launch(APP_KEY));
            session.set_auto_launch(APP_KEY, true).expect("turn on");
            assert!(session.auto_launch(APP_KEY));
            session.set_auto_launch(APP_KEY, false).expect("turn off");
            assert!(!session.auto_launch(APP_KEY));
        }));
        session.remove_manifest(path).expect("remove manifest");
        if let Err(panic) = outcome {
            std::panic::resume_unwind(panic);
        }
        assert!(!session.is_installed(APP_KEY));
        println!("add, toggle and remove all succeeded");

        drop(session);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
