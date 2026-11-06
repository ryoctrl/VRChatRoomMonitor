//! 画面右下のポップアップ、通知音、タスクバーの点滅。
//!
//! Windows 標準の通知は、インストールせずに実行すると発信者名が内部の ID になり、
//! 新しい発信元の通知が画面に出ないこともあった。確実に目に入るよう、独自のウィンドウで知らせる。

use crate::state::AppState;
use crate::window;
use monitor_core::monitor::{alert_sound, wants_alert};
use monitor_core::room::{FeedItem, FeedKind};
use monitor_core::sound::Sound;
use serde::Serialize;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Manager, Theme, UserAttentionType, WebviewUrl, WebviewWindowBuilder};
use windows_sys::Win32::Media::Audio::{
    PlaySoundW, SND_ALIAS, SND_ASYNC, SND_MEMORY, SND_NODEFAULT,
};

const POPUP_DURATION: Duration = Duration::from_secs(7);
const POPUP_WIDTH: f64 = 320.0;
const POPUP_HEIGHT: f64 = 72.0;
const POPUP_MARGIN: f64 = 12.0;
const POPUP_GAP: f64 = 8.0;
/// 同時に出す数の上限。超えた分は入退室ログで確認できる
const MAX_POPUPS: usize = 5;

/// 後から来た人や届いた招待を知らせる。
pub fn alert(app: &AppHandle, items: &[FeedItem]) {
    if items.is_empty() {
        return;
    }
    let (settings, world) = {
        let state = app.state::<AppState>();
        let inner = state.lock();
        (
            inner.settings.clone(),
            inner.monitor.world_name().to_string(),
        )
    };
    let wanted: Vec<&FeedItem> = items
        .iter()
        .filter(|item| wants_alert(item, settings.notify_leave))
        .collect();
    if wanted.is_empty() {
        return;
    }
    if settings.popup {
        for item in &wanted {
            show_popup(app, &message(item, &world));
        }
    }
    if settings.sound
        && let Some(sound) = alert_sound(&wanted, &settings)
    {
        play_sound(sound);
    }
    if let Some(main) = app.get_webview_window(window::MAIN)
        && !main.is_focused().unwrap_or(false)
    {
        let _ = main.request_user_attention(Some(UserAttentionType::Informational));
    }
}

/// ポップアップの画面に渡す内容。
#[derive(Serialize)]
struct Popup {
    title: String,
    body: String,
    kind: &'static str,
}

fn message(item: &FeedItem, world: &str) -> Popup {
    match item.kind {
        FeedKind::Join => Popup {
            title: format!("入室 · {world}"),
            body: format!("{} さんが入室しました", item.name),
            kind: "join",
        },
        FeedKind::Leave => Popup {
            title: format!("退室 · {world}"),
            body: format!("{} さんが退室しました", item.name),
            kind: "leave",
        },
        _ => Popup {
            title: "通知".to_string(),
            body: item.text.clone(),
            kind: "notice",
        },
    }
}

/// 閉じてもトレイで動き続けることを、最初の 1 回だけ案内する。
pub fn show_tray_hint_once(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.lock().settings.tray_hint_shown {
        return;
    }
    state.update_settings(|settings| settings.tray_hint_shown = true);
    // ウィンドウのイベント処理の中で WebView を作ると固まるので、別のスレッドで行う
    let app = app.clone();
    thread::spawn(move || {
        show_popup(
            &app,
            &Popup {
                title: window::APP_NAME.to_string(),
                body: "トレイで監視を続けています。終了はトレイのアイコンを右クリック".to_string(),
                kind: "info",
            },
        );
    });
}

/// 新しい版を見つけたことを知らせる。
pub fn show_update_available(app: &AppHandle, version: &str) {
    show_popup(
        app,
        &Popup {
            title: "更新があります".to_string(),
            body: format!("新しい版 {version} を利用できます"),
            kind: "info",
        },
    );
}

fn show_popup(app: &AppHandle, popup: &Popup) {
    let Some(slot) = (0..MAX_POPUPS).find(|slot| {
        app.get_webview_window(&window::popup_label(*slot))
            .is_none()
    }) else {
        return;
    };
    let Some((x, y)) = popup_position(app, slot) else {
        return;
    };
    let Ok(data) = serde_json::to_string(popup) else {
        return;
    };
    let result = WebviewWindowBuilder::new(
        app,
        window::popup_label(slot),
        WebviewUrl::App("popup.html".into()),
    )
    .title(format!("通知: {}", popup.body))
    .inner_size(POPUP_WIDTH, POPUP_HEIGHT)
    .position(x, y)
    .decorations(false)
    .resizable(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .focused(false)
    .theme(Some(Theme::Dark))
    .initialization_script(format!("window.__POPUP__ = {data};"))
    .build();
    match result {
        Ok(created) => {
            thread::spawn(move || {
                thread::sleep(POPUP_DURATION);
                let _ = created.destroy();
            });
        }
        Err(error) => eprintln!("failed to show a popup: {error}"),
    }
}

/// メインモニターの右下から順に積み上げる位置（タスクバーを除いた領域の中）。
fn popup_position(app: &AppHandle, slot: usize) -> Option<(f64, f64)> {
    let monitor = app.primary_monitor().ok().flatten()?;
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let right = f64::from(area.position.x) / scale + f64::from(area.size.width) / scale;
    let bottom = f64::from(area.position.y) / scale + f64::from(area.size.height) / scale;
    let stacked = (POPUP_HEIGHT + POPUP_GAP) * slot as f64;
    Some((
        right - POPUP_WIDTH - POPUP_MARGIN,
        bottom - POPUP_HEIGHT - POPUP_MARGIN - stacked,
    ))
}

/// 音を鳴らす。音を鳴らせない環境では失敗するだけで、他の通知には影響しない。
pub fn play_sound(sound: Sound) {
    if let Some(wav) = sound.wav() {
        // SAFETY: SND_MEMORY ではポインタを WAV データの先頭として扱う。データは終了まで保持され、
        // 非同期の再生中も有効。モジュールハンドルは不要。
        unsafe {
            PlaySoundW(
                wav.as_ptr().cast(),
                std::ptr::null_mut(),
                SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
            );
        }
    } else if sound == Sound::Windows {
        let alias: Vec<u16> = "Notification.Default\0".encode_utf16().collect();
        // SAFETY: alias は NUL 終端の UTF-16 文字列。エイリアス指定ではモジュールハンドルは不要。
        unsafe {
            PlaySoundW(
                alias.as_ptr(),
                std::ptr::null_mut(),
                SND_ALIAS | SND_ASYNC | SND_NODEFAULT,
            );
        }
    }
}
