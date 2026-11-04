//! アプリの更新。GitHub Releases の更新情報を定期的に確認し、利用者の操作で更新する。
//!
//! VR の最中に勝手に再起動しないよう、自動で行うのは確認までにしている。
//! 更新ファイルの署名は、インストールの前に Tauri の updater が検証する。

use crate::notifier;
use crate::state::{self, AppState};
use serde::Serialize;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::UpdaterExt;

const FIRST_CHECK_DELAY: Duration = Duration::from_secs(15);
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const RELEASES_URL: &str = "https://github.com/ryoctrl/VRChatRoomMonitor/releases/latest";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum UpdateStatus {
    #[default]
    None,
    Available {
        version: String,
        /// インストーラで入れた場合だけ、アプリの中から更新できる
        installable: bool,
    },
    Installing {
        version: String,
        percent: Option<u8>,
    },
    Failed {
        version: String,
        message: String,
    },
}

/// 定期的な確認を始める。
pub fn start(app: &AppHandle) {
    let app = app.clone();
    thread::spawn(move || {
        thread::sleep(FIRST_CHECK_DELAY);
        loop {
            let enabled = app.state::<AppState>().lock().settings.check_updates;
            if enabled {
                tauri::async_runtime::block_on(check(&app));
            }
            thread::sleep(CHECK_INTERVAL);
        }
    });
}

/// 設定でオンにされたときなどに、すぐ確認する。
pub fn check_soon(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { check(&app).await });
}

/// 確認をオフにしたら、案内も取り下げる。
pub fn clear(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut inner = state.lock();
    if !matches!(inner.update, UpdateStatus::Installing { .. }) {
        inner.update = UpdateStatus::None;
    }
}

async fn check(app: &AppHandle) {
    let Ok(updater) = app.updater() else {
        return;
    };
    // 通信できないときや新しい版が無いときは、何も表示せず次の確認を待つ
    let Ok(Some(update)) = updater.check().await else {
        return;
    };
    let version = update.version.clone();
    let state = app.state::<AppState>();
    *state.pending_update.lock().expect("lock is never poisoned") = Some(update);
    {
        let mut inner = state.lock();
        if matches!(inner.update, UpdateStatus::Installing { .. }) {
            return;
        }
        let already_shown = matches!(
            &inner.update,
            UpdateStatus::Available { version: shown, .. } if *shown == version
        );
        inner.update = UpdateStatus::Available {
            version: version.clone(),
            installable: is_installed(),
        };
        if already_shown {
            return;
        }
    }
    state::publish(app);
    notifier::show_update_available(app, &version);
}

/// インストーラで入れた場合は、exe の隣にアンインストーラがある。
fn is_installed() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("uninstall.exe").exists()))
        .unwrap_or(false)
}

/// 更新をダウンロードしてインストールする。Windows では、インストーラの起動時にアプリが終了し、
/// インストール後に新しい版が起動する。
pub async fn install(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let update = state
        .pending_update
        .lock()
        .expect("lock is never poisoned")
        .clone()
        .ok_or_else(|| "更新の情報がありません".to_string())?;
    let version = update.version.clone();
    set_status(
        app,
        UpdateStatus::Installing {
            version: version.clone(),
            percent: None,
        },
    );

    let progress_app = app.clone();
    let progress_version = version.clone();
    let mut downloaded: u64 = 0;
    let mut last_percent = None;
    let result = update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                let percent = total.map(|total| (downloaded * 100 / total.max(1)).min(100) as u8);
                if percent != last_percent {
                    last_percent = percent;
                    set_status(
                        &progress_app,
                        UpdateStatus::Installing {
                            version: progress_version.clone(),
                            percent,
                        },
                    );
                }
            },
            || {},
        )
        .await;

    result.map_err(|error| {
        let message = error.to_string();
        set_status(
            app,
            UpdateStatus::Failed {
                version,
                message: message.clone(),
            },
        );
        message
    })
}

fn set_status(app: &AppHandle, status: UpdateStatus) {
    app.state::<AppState>().lock().update = status;
    state::publish(app);
}

/// インストーラを使っていない場合は、配布ページを開く。
pub fn open_releases(app: &AppHandle) -> Result<(), String> {
    app.opener()
        .open_url(RELEASES_URL, None::<&str>)
        .map_err(|error| error.to_string())
}
