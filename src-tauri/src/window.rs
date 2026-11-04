//! ウィンドウの作成と表示。

use crate::state::AppState;
use tauri::{AppHandle, Manager, Theme, WebviewUrl, WebviewWindowBuilder};

pub const APP_NAME: &str = "VRChat Room Monitor";
pub const MAIN: &str = "main";
pub const TRAY: &str = "main";
const LICENSES: &str = "licenses";
const POPUP_PREFIX: &str = "popup-";

pub fn popup_label(slot: usize) -> String {
    format!("{POPUP_PREFIX}{slot}")
}

/// 位置とサイズを次回に引き継ぐのはメインウィンドウだけ。
pub fn remembers_state(label: &str) -> bool {
    label == MAIN
}

pub fn create_main(app: &AppHandle) -> tauri::Result<()> {
    let (title, always_on_top) = {
        let state = app.state::<AppState>();
        let inner = state.lock();
        (inner.title(), inner.settings.always_on_top)
    };
    WebviewWindowBuilder::new(app, MAIN, WebviewUrl::default())
        .title(title)
        .inner_size(440.0, 700.0)
        .min_inner_size(340.0, 440.0)
        .theme(Some(Theme::Dark))
        .always_on_top(always_on_top)
        .build()?;
    Ok(())
}

/// メインウィンドウを前面に出す。閉じていれば作り直す。
pub fn open_main(app: &AppHandle) {
    // Windows ではイベント処理の中で WebView を作ると固まるので、別のタスクで行う
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = match app.get_webview_window(MAIN) {
            Some(window) => window
                .unminimize()
                .and_then(|()| window.show())
                .and_then(|()| window.set_focus()),
            None => create_main(&app),
        };
        if let Err(error) = result {
            eprintln!("failed to open the window: {error}");
        }
    });
}

pub fn open_licenses(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(LICENSES) {
        return window.set_focus();
    }
    WebviewWindowBuilder::new(
        app,
        LICENSES,
        WebviewUrl::App("third-party-notices.html".into()),
    )
    .title(format!("ライセンス - {APP_NAME}"))
    .inner_size(640.0, 700.0)
    .build()?;
    Ok(())
}
