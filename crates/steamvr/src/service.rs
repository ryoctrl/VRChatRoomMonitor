//! SteamVR の起動と終了に追従するスレッド。
//!
//! SteamVR が起動していれば接続し、登録ファイルを渡して一覧に載せる。
//! このアプリを SteamVR の同時起動で動かすかどうかは、SteamVR 側の設定（自動起動）が決める。

use crate::Error;
use crate::api::{ConnectionKind, OpenVr, Session};
use crate::manifest::{self, APP_KEY};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};

const POLL_INTERVAL: Duration = Duration::from_secs(1);
const CONNECT_RETRY: Duration = Duration::from_secs(5);
const CHECK_INTERVAL: Duration = Duration::from_secs(5);
/// 終了要求の直後はまだ SteamVR のプロセスが残っているので、再接続までしばらく待つ
const AFTER_QUIT_DELAY: Duration = Duration::from_secs(30);
const REPLY_TIMEOUT: Duration = Duration::from_secs(60);
const SERVER_PROCESS: &str = "vrserver.exe";

pub struct Config {
    pub data_dir: PathBuf,
    pub exe_path: PathBuf,
    pub icon_png: &'static [u8],
    pub launch_argument: &'static str,
    /// SteamVR の終了時にこのアプリも終了する（SteamVR から起動された場合）
    pub exit_with_steamvr: bool,
    /// 接続時に一覧へ自動で登録する（利用者が一覧から削除した場合と開発中は false）
    pub auto_register: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// SteamVR がインストールされている
    pub installed: bool,
    /// 起動中の SteamVR に接続している
    pub connected: bool,
    /// SteamVR の一覧に載っている（未接続のあいだは最後に確認した値）
    pub listed: bool,
    /// SteamVR と同時に起動する（未接続のあいだは最後に確認した値）
    pub autostart: bool,
}

enum Command {
    SetAutostart(bool, Sender<Result<(), Error>>),
    Unregister(Sender<Result<(), Error>>),
}

#[derive(Clone)]
pub struct Handle {
    commands: Sender<Command>,
}

impl Handle {
    /// 同時起動を切り替える。SteamVR が止まっていても設定できる。処理が終わるまで待つ。
    pub fn set_autostart(&self, enabled: bool) -> Result<(), Error> {
        self.request(|reply| Command::SetAutostart(enabled, reply))
    }

    /// SteamVR の一覧から外す。処理が終わるまで待つ。
    pub fn unregister(&self) -> Result<(), Error> {
        self.request(Command::Unregister)
    }

    fn request(
        &self,
        command: impl FnOnce(Sender<Result<(), Error>>) -> Command,
    ) -> Result<(), Error> {
        let stopped = || Error::Runtime("SteamVR との連携が停止しています".into());
        let (reply, result) = mpsc::channel();
        self.commands.send(command(reply)).map_err(|_| stopped())?;
        result.recv_timeout(REPLY_TIMEOUT).map_err(|_| stopped())?
    }
}

/// 連携スレッドを開始する。`on_status` は状態の変化時に、`on_quit` はアプリも終了すべきときに呼ばれる。
pub fn spawn(
    config: Config,
    on_status: impl Fn(&Status) + Send + 'static,
    on_quit: impl Fn() + Send + 'static,
) -> Handle {
    let (commands, received) = mpsc::channel();
    thread::spawn(move || {
        Worker {
            api: load_api(&config),
            config,
            session: None,
            status: Status::default(),
            on_status,
            on_quit,
            next_connect: Instant::now(),
            next_check: Instant::now(),
        }
        .run(&received);
    });
    Handle { commands }
}

fn load_api(config: &Config) -> Result<Rc<OpenVr>, Error> {
    OpenVr::load(&config.data_dir).map(Rc::new)
}

struct Worker<S, Q> {
    config: Config,
    api: Result<Rc<OpenVr>, Error>,
    session: Option<Session>,
    status: Status,
    on_status: S,
    on_quit: Q,
    next_connect: Instant,
    next_check: Instant,
}

impl<S: Fn(&Status), Q: Fn()> Worker<S, Q> {
    fn run(mut self, commands: &Receiver<Command>) {
        if let Ok(api) = &self.api {
            self.status.installed = api.is_runtime_installed();
            (self.on_status)(&self.status);
        }
        loop {
            match commands.recv_timeout(POLL_INTERVAL) {
                Ok(Command::SetAutostart(enabled, reply)) => {
                    let _ = reply.send(self.set_autostart(enabled));
                }
                Ok(Command::Unregister(reply)) => {
                    let _ = reply.send(self.unregister());
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            self.tick();
        }
    }

    fn tick(&mut self) {
        let now = Instant::now();
        let Some(session) = &self.session else {
            if now >= self.next_connect {
                self.next_connect = now + CONNECT_RETRY;
                if is_server_running() {
                    self.connect();
                }
            }
            return;
        };
        if session.poll_quit() {
            session.acknowledge_quit();
            self.disconnect(AFTER_QUIT_DELAY);
            self.quit_if_requested();
            return;
        }
        if now >= self.next_check {
            self.next_check = now + CHECK_INTERVAL;
            if is_server_running() {
                // SteamVR の設定画面で切り替えられた場合に追従する
                self.refresh_status();
            } else {
                self.disconnect(CONNECT_RETRY);
                self.quit_if_requested();
            }
        }
    }

    fn quit_if_requested(&self) {
        if self.config.exit_with_steamvr {
            (self.on_quit)();
        }
    }

    fn connect(&mut self) {
        let Ok(api) = &self.api else {
            return;
        };
        // 接続できなければ次の周期で再試行する
        let Ok(session) = api.connect(ConnectionKind::Background) else {
            return;
        };
        self.status.installed = true;
        self.status.connected = true;
        self.session = Some(session);
        if self.config.auto_register {
            // 失敗しても、チェックを入れたときに改めて登録するので続行する
            let _ = self.register();
        }
        self.refresh_status();
        (self.on_status)(&self.status);
    }

    fn disconnect(&mut self, delay: Duration) {
        self.session = None;
        self.status.connected = false;
        self.next_connect = Instant::now() + delay;
        (self.on_status)(&self.status);
    }

    fn refresh_status(&mut self) {
        let Some(session) = &self.session else {
            return;
        };
        let listed = session.is_installed(APP_KEY);
        let autostart = listed && session.auto_launch(APP_KEY);
        if (listed, autostart) != (self.status.listed, self.status.autostart) {
            self.status.listed = listed;
            self.status.autostart = autostart;
            (self.on_status)(&self.status);
        }
    }

    fn register(&self) -> Result<(), Error> {
        let session = self.session.as_ref().ok_or(Error::NotRunning)?;
        session.add_manifest(&self.prepare_manifest()?)
    }

    fn prepare_manifest(&self) -> Result<String, Error> {
        let io_error = |e: std::io::Error| Error::Runtime(e.to_string());
        let dir: &Path = &self.config.data_dir;
        std::fs::create_dir_all(dir).map_err(io_error)?;
        let image = dir.join("icon.png");
        std::fs::write(&image, self.config.icon_png).map_err(io_error)?;
        let path = manifest::write(
            dir,
            &self.config.exe_path,
            &image,
            self.config.launch_argument,
        )
        .map_err(io_error)?;
        path.to_str()
            .map(str::to_string)
            .ok_or_else(|| Error::Runtime("登録ファイルのパスを扱えません".into()))
    }

    /// 接続中はその接続を使い、未接続なら設定の変更用に短時間だけ接続する。
    fn with_session<T>(
        &mut self,
        action: impl FnOnce(&Session) -> Result<T, Error>,
    ) -> Result<T, Error> {
        if let Some(session) = &self.session {
            return action(session);
        }
        let api = self.api.as_ref().map_err(Clone::clone)?;
        let session = api.connect(ConnectionKind::Utility)?;
        action(&session)
    }

    fn set_autostart(&mut self, enabled: bool) -> Result<(), Error> {
        let manifest = self.prepare_manifest()?;
        self.with_session(|session| {
            session.add_manifest(&manifest)?;
            session.set_auto_launch(APP_KEY, enabled)
        })?;
        self.status.installed = true;
        self.status.listed = true;
        self.status.autostart = enabled;
        (self.on_status)(&self.status);
        Ok(())
    }

    fn unregister(&mut self) -> Result<(), Error> {
        let manifest = self.prepare_manifest()?;
        self.with_session(|session| {
            if session.is_installed(APP_KEY) {
                session.set_auto_launch(APP_KEY, false)?;
                session.remove_manifest(&manifest)?;
            }
            Ok(())
        })?;
        self.config.auto_register = false;
        self.status.listed = false;
        self.status.autostart = false;
        (self.on_status)(&self.status);
        Ok(())
    }
}

/// SteamVR のサーバーが動いているかを、接続を試みずに確かめる。
fn is_server_running() -> bool {
    // SAFETY: スナップショットのハンドルはこの関数の中だけで使って閉じる。
    // entry は書き込み可能で、dwSize に実際の大きさを設定している。
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut found = false;
        let mut has_entry = Process32FirstW(snapshot, &mut entry) != 0;
        while has_entry && !found {
            let length = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(0);
            found = String::from_utf16_lossy(&entry.szExeFile[..length])
                .eq_ignore_ascii_case(SERVER_PROCESS);
            has_entry = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
        found
    }
}
