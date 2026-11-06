//! 画面から呼ばれるコマンド。

use crate::notifier;
use crate::state::{self, AppState, Snapshot};
use crate::updates;
use crate::window;
use monitor_core::sound::Sound;
use serde::Deserialize;
use tauri::{AppHandle, Manager, State, WebviewWindow};

#[tauri::command]
pub fn get_snapshot(state: State<'_, AppState>) -> Snapshot {
    state.lock().snapshot()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    always_on_top: Option<bool>,
    popup: Option<bool>,
    sound: Option<bool>,
    notify_leave: Option<bool>,
    check_updates: Option<bool>,
    current_world_only: Option<bool>,
    fixed_title: Option<bool>,
    sound_join: Option<Sound>,
    sound_leave: Option<Sound>,
    sound_notice: Option<Sound>,
}

#[tauri::command]
pub fn update_settings(app: AppHandle, state: State<'_, AppState>, patch: SettingsPatch) {
    state.update_settings(|settings| {
        settings.always_on_top = patch.always_on_top.unwrap_or(settings.always_on_top);
        settings.popup = patch.popup.unwrap_or(settings.popup);
        settings.sound = patch.sound.unwrap_or(settings.sound);
        settings.notify_leave = patch.notify_leave.unwrap_or(settings.notify_leave);
        settings.check_updates = patch.check_updates.unwrap_or(settings.check_updates);
        settings.current_world_only = patch
            .current_world_only
            .unwrap_or(settings.current_world_only);
        settings.fixed_title = patch.fixed_title.unwrap_or(settings.fixed_title);
        settings.sound_join = patch.sound_join.unwrap_or(settings.sound_join);
        settings.sound_leave = patch.sound_leave.unwrap_or(settings.sound_leave);
        settings.sound_notice = patch.sound_notice.unwrap_or(settings.sound_notice);
    });
    match patch.check_updates {
        Some(true) => updates::check_soon(&app),
        Some(false) => updates::clear(&app),
        None => {}
    }
    if let (Some(always_on_top), Some(main)) =
        (patch.always_on_top, app.get_webview_window(window::MAIN))
    {
        let _ = main.set_always_on_top(always_on_top);
    }
    state::publish(&app);
}

/// 選んだ音を試しに鳴らす。
#[tauri::command]
pub fn preview_sound(sound: Sound) {
    notifier::play_sound(sound);
}

fn steamvr_handle(app: &AppHandle) -> Result<steamvr::Handle, String> {
    app.state::<AppState>()
        .steamvr
        .get()
        .cloned()
        .ok_or_else(|| "SteamVR との連携は無効です".to_string())
}

/// SteamVR への接続には数秒かかることがあるので、画面を止めないよう別スレッドで待つ。
async fn run_steamvr(
    action: impl FnOnce() -> Result<(), steamvr::Error> + Send + 'static,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(action)
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn set_steamvr_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    let handle = steamvr_handle(&app)?;
    run_steamvr(move || handle.set_autostart(enabled)).await?;
    let state = app.state::<AppState>();
    state.update_settings(|settings| settings.steamvr_removed = false);
    Ok(())
}

#[tauri::command]
pub async fn remove_steamvr_entry(app: AppHandle) -> Result<(), String> {
    let handle = steamvr_handle(&app)?;
    run_steamvr(move || handle.unregister()).await?;
    let state = app.state::<AppState>();
    state.update_settings(|settings| settings.steamvr_removed = true);
    Ok(())
}

// Windows では同期コマンドの中で WebView を作ると固まるので、async にしている
#[tauri::command]
pub async fn open_licenses(app: AppHandle) -> Result<(), String> {
    window::open_licenses(&app).map_err(|error| error.to_string())
}

/// ポップアップがクリックされたら閉じて、メインウィンドウを前面に出す。
#[tauri::command]
pub async fn dismiss_popup(app: AppHandle, webview: WebviewWindow) -> Result<(), String> {
    window::open_main(&app);
    webview.destroy().map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    updates::install(&app).await
}

#[tauri::command]
pub async fn open_releases(app: AppHandle) -> Result<(), String> {
    updates::open_releases(&app)
}
