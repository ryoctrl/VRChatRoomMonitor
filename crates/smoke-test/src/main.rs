//! ビルドした exe を実際に起動して確認する。CI でも使う。
//!
//! 実行: `cargo run -p smoke-test -- <exe のパス>`
//!
//! 同梱ライブラリの読み込み、ログの表示、後から来た人の反映とポップアップ、閉じてもトレイで
//! 動き続けること、二重起動の防止、再表示、終了を確かめる。ログと設定は一時フォルダを使い、SteamVR にも接続しない。
//! 実際のアプリの通知と紛らわしいので、通知音はオフにして実行する。

use chrono::Local;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM, TRUE};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
    PostMessageW, WM_CLOSE,
};

const TIMEOUT: Duration = Duration::from_secs(60);
const APP_NAME: &str = "VRChat Room Monitor";
const WORLD: &str = "スモークテスト";
const USER: &str = "usr_00000000-0000-0000-0000-00000000000";
const LOG_FILE: &str = "output_log_2026-01-01_00-00-00.txt";

fn process_image(pid: u32) -> Option<PathBuf> {
    // SAFETY: ハンドルはこの関数の中だけで使って閉じる。バッファの大きさは size で渡している。
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut buffer = [0u16; 1024];
        let mut size = buffer.len() as u32;
        let ok =
            QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut size);
        CloseHandle(process);
        (ok != 0).then(|| PathBuf::from(String::from_utf16_lossy(&buffer[..size as usize])))
    }
}

struct Search<'a> {
    exe: &'a Path,
    found: Vec<(HWND, String)>,
}

unsafe extern "system" fn visit_window(window: HWND, context: LPARAM) -> i32 {
    // SAFETY: context は app_windows が渡した Search で、列挙の間は有効。window は列挙で渡された有効な値。
    unsafe {
        let search = &mut *(context as *mut Search);
        let mut pid = 0;
        GetWindowThreadProcessId(window, &mut pid);
        if IsWindowVisible(window) != 0 && process_image(pid).as_deref() == Some(search.exe) {
            let mut title = [0u16; 256];
            let length = GetWindowTextW(window, title.as_mut_ptr(), title.len() as i32);
            let title = String::from_utf16_lossy(&title[..length.max(0) as usize]);
            search.found.push((window, title));
        }
    }
    TRUE
}

/// exe の表示中のウィンドウを、題名つきで列挙する。
fn app_windows(exe: &Path) -> Vec<(HWND, String)> {
    let mut search = Search {
        exe,
        found: Vec::new(),
    };
    // SAFETY: search は列挙が終わるまで有効で、コールバックは同じ型として読み出す。
    unsafe { EnumWindows(Some(visit_window), &mut search as *mut Search as LPARAM) };
    search.found
}

fn window_with_title(exe: &Path, part: &str) -> Option<HWND> {
    app_windows(exe)
        .into_iter()
        .find(|(_, title)| title.contains(part))
        .map(|(window, _)| window)
}

fn wait_until<T>(description: &str, mut condition: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline {
        if let Some(value) = condition() {
            println!("ok: {description}");
            return value;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    panic!("timeout: {description}");
}

fn log_line(message: &str) -> String {
    format!(
        "{} Debug      -  {message}\n",
        Local::now().format("%Y.%m.%d %H:%M:%S")
    )
}

fn append(path: &Path, text: &str) {
    let mut file = File::options()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    file.write_all(text.as_bytes()).unwrap();
}

struct App<'a> {
    exe: &'a Path,
    log_dir: PathBuf,
    data_dir: PathBuf,
    webview_dir: PathBuf,
}

impl App<'_> {
    fn command(&self) -> Command {
        let mut command = Command::new(self.exe);
        command
            .arg("--log-dir")
            .arg(&self.log_dir)
            .arg("--data-dir")
            .arg(&self.data_dir)
            .arg("--no-steamvr")
            .arg("--no-update-check")
            .env("WEBVIEW2_USER_DATA_FOLDER", &self.webview_dir);
        command
    }

    fn run_to_end(&self, extra: &[&str]) -> i32 {
        let mut child = self.command().args(extra).spawn().unwrap();
        wait_for_exit(&mut child)
    }
}

fn wait_for_exit(child: &mut Child) -> i32 {
    wait_until("process exits", || {
        child
            .try_wait()
            .unwrap()
            .map(|status| status.code().unwrap_or(-1))
    })
}

fn run(exe: &Path, root: &Path) {
    let app = App {
        exe,
        log_dir: root.join("logs"),
        data_dir: root.join("data"),
        webview_dir: root.join("webview"),
    };
    fs::create_dir_all(&app.log_dir).unwrap();
    fs::create_dir_all(&app.data_dir).unwrap();
    let settings = app.data_dir.join("settings.json");
    fs::write(&settings, r#"{"sound": false}"#).unwrap();

    let self_check = Command::new(exe).arg("--self-check").status().unwrap();
    assert_eq!(self_check.code(), Some(0), "--self-check failed");
    println!("ok: bundled libraries load");

    let log = app.log_dir.join(LOG_FILE);
    append(
        &log,
        &[
            log_line(&format!("[Behaviour] Entering Room: {WORLD}")),
            log_line(&format!(
                "[Behaviour] Joining wrld_00000000-0000-0000-0000-000000000000:1~hidden({USER}1)~region(jp)"
            )),
            log_line(&format!("[Behaviour] OnPlayerJoined Alice ({USER}2)")),
            log_line(&format!("[Behaviour] OnPlayerJoined Me ({USER}1)")),
            log_line("[Behaviour] Initialized PlayerAPI \"Me\" is local"),
        ]
        .concat(),
    );

    let mut first = app.command().spawn().unwrap();
    wait_until("log is shown in the window title", || {
        window_with_title(exe, &format!("2人 · {WORLD}"))
    });

    // 表示名には記号も入るので、通知の XML を壊す文字でも止まらないことを確かめる
    append(
        &log,
        &log_line(&format!("[Behaviour] OnPlayerJoined <B&b> ({USER}3)")),
    );
    let main = wait_until("later join is reflected", || {
        window_with_title(exe, &format!("3人 · {WORLD}"))
    });
    let popup = wait_until("popup is shown with the raw name", || {
        window_with_title(exe, "通知: <B&b> さんが入室しました")
    });
    // SAFETY: 引数の無い問い合わせで、いつ呼んでもよい。
    assert_ne!(
        unsafe { GetForegroundWindow() },
        popup,
        "popup must not take the focus"
    );
    println!("ok: popup does not take the focus");
    wait_until("popup closes by itself", || {
        window_with_title(exe, "通知:").is_none().then_some(())
    });

    assert_eq!(
        app.run_to_end(&["--steamvr"]),
        0,
        "second launch from SteamVR"
    );
    assert!(first.try_wait().unwrap().is_none(), "first instance exited");
    println!("ok: second launch from SteamVR exits quietly");

    // SAFETY: main は列挙で得たウィンドウ。閉じられていても PostMessageW が失敗するだけ。
    unsafe { PostMessageW(main, WM_CLOSE, 0, 0) };
    // 題名で探す（Tauri の内部にも、画面に見えない補助ウィンドウがあるため）
    wait_until("window closes", || {
        window_with_title(exe, APP_NAME).is_none().then_some(())
    });
    std::thread::sleep(Duration::from_secs(1));
    assert!(
        first.try_wait().unwrap().is_none(),
        "closing the window must not exit the app"
    );
    println!("ok: keeps running in the tray after closing the window");

    assert_eq!(app.run_to_end(&[]), 0, "second launch");
    wait_until("second launch reopens the window", || {
        window_with_title(exe, WORLD)
    });

    assert_eq!(app.run_to_end(&["--quit"]), 0, "--quit");
    assert_eq!(wait_for_exit(&mut first), 0, "first instance exit code");
    println!("ok: exits with --quit");

    // アプリが保存し直した内容で、事前の設定を読み込んだことと、保存できることを確かめる
    let saved = fs::read_to_string(&settings).unwrap();
    assert!(
        saved.contains(r#""sound": false"#),
        "sound setting not loaded: {saved}"
    );
    assert!(
        saved.contains(r#""trayHintShown": true"#),
        "settings not saved: {saved}"
    );
    println!("ok: settings are loaded and saved, with the sound off");
}

fn main() {
    let exe = std::env::args().nth(1).expect("usage: smoke-test <exe>");
    let exe = fs::canonicalize(exe).expect("exe not found");
    // canonicalize は \\?\ を付けるが、プロセスから得るパスには付かないので揃える
    let exe = PathBuf::from(exe.to_string_lossy().trim_start_matches(r"\\?\"));
    let root = std::env::temp_dir().join(format!("vrm-smoke-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    run(&exe, &root);
    let _ = fs::remove_dir_all(&root);
    println!("all checks passed");
}
