//! VRChat Room Monitor: VRChat のログを監視し、現在のルームの状況と入退室の記録を表示する。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod notifier;
mod state;
mod updates;
mod window;

use monitor_core::cli::{self, Args};
use monitor_core::monitor::Monitor;
use monitor_core::settings::Settings;
use monitor_core::watcher::{self, Timing};
use state::{AppState, Inner};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent, WindowEvent};

const ICON_PNG: &[u8] = include_bytes!("../icons/128x128.png");

fn main() {
    let args = Args::parse(std::env::args().skip(1));
    if args.self_check {
        let code = match steamvr::self_check(&std::env::temp_dir().join("VRChatRoomMonitor")) {
            Ok(()) => 0,
            Err(_) => 3,
        };
        std::process::exit(code);
    }

    tauri::Builder::default()
        // 2 つ目の起動はここで引数を起動中のアプリに渡して終了する
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            let args = Args::parse(argv.into_iter().skip(1));
            if args.quit {
                app.exit(0);
            } else if !args.from_steamvr {
                window::open_main(app);
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_filter(window::remembers_state)
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::update_settings,
            commands::preview_sound,
            commands::set_steamvr_autostart,
            commands::remove_steamvr_entry,
            commands::open_licenses,
            commands::dismiss_popup,
            commands::install_update,
            commands::open_releases,
        ])
        .setup(move |app| setup(app.handle(), args).map_err(Into::into))
        .on_window_event(|window, event| {
            if window.label() == window::MAIN && matches!(event, WindowEvent::CloseRequested { .. })
            {
                notifier::show_tray_hint_once(window.app_handle());
            }
        })
        .build(tauri::generate_context!())
        .expect("failed to build the application")
        .run(|_app, event| {
            // ウィンドウを閉じても WebView だけを解放し、トレイで監視を続ける
            if let RunEvent::ExitRequested {
                code: None, api, ..
            } = event
            {
                api.prevent_exit();
            }
        });
}

fn setup(app: &AppHandle, args: Args) -> tauri::Result<()> {
    if args.quit {
        // 起動中のアプリが無かったので、何もせずに終わる
        app.exit(0);
        return Ok(());
    }
    let data_dir = match args.data_dir {
        Some(dir) => dir,
        None => app.path().app_data_dir()?,
    };
    let log_dir = match args.log_dir {
        Some(dir) => dir,
        None => default_log_dir(app)?,
    };
    let settings_path = data_dir.join("settings.json");
    let settings = Settings::load(&settings_path);

    app.manage(AppState {
        inner: Mutex::new(Inner {
            monitor: Monitor::default(),
            settings,
            steamvr: steamvr::Status::default(),
            update: updates::UpdateStatus::default(),
            app_version: app.package_info().version.to_string(),
        }),
        settings_path,
        pending_update: Mutex::new(None),
        steamvr: OnceLock::new(),
        watcher: OnceLock::new(),
    });

    build_tray(app)?;
    window::create_main(app)?;

    let handle = app.clone();
    let watcher = watcher::spawn(log_dir, Timing::default(), move |message| {
        let alerts = handle.state::<AppState>().lock().monitor.handle(message);
        state::publish(&handle);
        notifier::alert(&handle, &alerts);
    });
    let state = app.state::<AppState>();
    let _ = state.watcher.set(watcher);

    if !args.no_steamvr {
        let _ = state
            .steamvr
            .set(start_steamvr(app, data_dir, args.from_steamvr)?);
    }
    // 開発中の exe は更新しない
    if !cfg!(debug_assertions) && !args.no_update_check {
        updates::start(app);
    }
    Ok(())
}

fn start_steamvr(
    app: &AppHandle,
    data_dir: PathBuf,
    from_steamvr: bool,
) -> tauri::Result<steamvr::Handle> {
    let state = app.state::<AppState>();
    let config = steamvr::Config {
        data_dir,
        exe_path: std::env::current_exe()?,
        icon_png: ICON_PNG,
        launch_argument: cli::FROM_STEAMVR,
        exit_with_steamvr: from_steamvr,
        // 開発中の exe を実際の SteamVR に登録しない
        auto_register: !cfg!(debug_assertions) && !state.lock().settings.steamvr_removed,
    };
    let on_status = {
        let app = app.clone();
        move |status: &steamvr::Status| {
            app.state::<AppState>().lock().steamvr = status.clone();
            state::publish(&app);
        }
    };
    let on_quit = {
        let app = app.clone();
        move || app.exit(0)
    };
    Ok(steamvr::spawn(config, on_status, on_quit))
}

fn default_log_dir(app: &AppHandle) -> tauri::Result<PathBuf> {
    Ok(app
        .path()
        .home_dir()?
        .join("AppData")
        .join("LocalLow")
        .join("VRChat")
        .join("VRChat"))
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "開く", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "終了", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    TrayIconBuilder::with_id(window::TRAY)
        .icon(Image::from_bytes(ICON_PNG)?)
        .tooltip(window::APP_NAME)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => window::open_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                window::open_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}
