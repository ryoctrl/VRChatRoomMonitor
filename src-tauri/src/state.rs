//! アプリ全体の状態と、画面への反映。

use crate::updates::UpdateStatus;
use crate::window;
use monitor_core::monitor::{Monitor, RoomView};
use monitor_core::settings::Settings;
use monitor_core::watcher::WatcherHandle;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::Update;

pub const SNAPSHOT_EVENT: &str = "snapshot";

pub struct AppState {
    pub inner: Mutex<Inner>,
    pub settings_path: PathBuf,
    /// 確認で見つかった更新。利用者が更新を選ぶまで保持する
    pub pending_update: Mutex<Option<Update>>,
    pub steamvr: OnceLock<steamvr::Handle>,
    /// 保持している間だけログの監視が続く
    pub watcher: OnceLock<WatcherHandle>,
}

pub struct Inner {
    pub monitor: Monitor,
    pub settings: Settings,
    pub steamvr: steamvr::Status,
    pub update: UpdateStatus,
    pub app_version: String,
}

/// 画面に渡す現在の状態。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    #[serde(flatten)]
    room: RoomView,
    settings: Settings,
    steamvr: steamvr::Status,
    update: UpdateStatus,
    app_version: String,
}

impl Inner {
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            room: self.monitor.view(),
            settings: self.settings.clone(),
            steamvr: self.steamvr.clone(),
            update: self.update.clone(),
            app_version: self.app_version.clone(),
        }
    }

    pub fn title(&self) -> String {
        match self.monitor.summary() {
            Some(summary) => format!("{summary} - {}", window::APP_NAME),
            None => window::APP_NAME.to_string(),
        }
    }
}

impl AppState {
    pub fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().expect("state lock is never poisoned")
    }

    /// 設定を変更して保存する。保存に失敗しても、動作中の設定は変更したまま続ける。
    pub fn update_settings(&self, change: impl FnOnce(&mut Settings)) {
        let mut inner = self.lock();
        change(&mut inner.settings);
        let _ = inner.settings.save(&self.settings_path);
    }
}

/// 現在の状態を画面、ウィンドウの題名、トレイの説明に反映する。
pub fn publish(app: &AppHandle) {
    let (snapshot, title) = {
        let state = app.state::<AppState>();
        let inner = state.lock();
        (inner.snapshot(), inner.title())
    };
    let _ = app.emit(SNAPSHOT_EVENT, &snapshot);
    if let Some(main) = app.get_webview_window(window::MAIN) {
        let _ = main.set_title(&title);
    }
    if let Some(tray) = app.tray_by_id(window::TRAY) {
        let _ = tray.set_tooltip(Some(&title));
    }
}
