//! 最新のログファイルを読み続け、解析したイベントを通知する。
//!
//! Windows では、書き込み中のファイルの更新通知は遅れることがあるため、
//! ファイル変更の通知には頼らず、短い間隔で続きを読む。

use crate::parser::{TimedEvent, parse_line};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

const READ_CHUNK: usize = 1 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchMessage {
    /// 読み始めたファイル。replay は起動前からあったファイルであること
    File { path: PathBuf, replay: bool },
    Events {
        events: Vec<TimedEvent>,
        replay: bool,
    },
    /// 起動前の分を読み終えた
    ReplayDone,
    /// ログファイルが見つからない
    NoLog { dir: PathBuf },
    /// 読み込みに失敗した（次の周期で再試行する）
    Error { message: String },
}

#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub poll: Duration,
    pub rescan: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            poll: Duration::from_millis(500),
            rescan: Duration::from_secs(3),
        }
    }
}

/// 監視を止めるためのハンドル。破棄すると監視スレッドは次の周期で終わる。
pub struct WatcherHandle {
    _stop: Sender<()>,
}

pub fn spawn(
    log_dir: PathBuf,
    timing: Timing,
    on_message: impl FnMut(WatchMessage) + Send + 'static,
) -> WatcherHandle {
    let (stop, stopped) = mpsc::channel::<()>();
    thread::spawn(move || {
        let mut watcher = Watcher::new(log_dir, on_message);
        let mut last_scan: Option<Instant> = None;
        loop {
            if last_scan.is_none_or(|scanned| scanned.elapsed() >= timing.rescan) {
                last_scan = Some(Instant::now());
                watcher.scan();
            }
            watcher.read_new();
            match stopped.recv_timeout(timing.poll) {
                Err(RecvTimeoutError::Timeout) => {}
                _ => break,
            }
        }
    });
    WatcherHandle { _stop: stop }
}

/// ファイル名に起動日時が入っているので、名前の順で最新が決まる。
pub fn find_latest_log(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("output_log_") && name.ends_with(".txt"))
        })
        .max_by_key(|path| path.file_name().map(ToOwned::to_owned))
}

struct OpenLog {
    path: PathBuf,
    file: File,
    position: u64,
    /// 行の途中まで読んだ分
    pending: Vec<u8>,
}

struct Watcher<F> {
    dir: PathBuf,
    on_message: F,
    log: Option<OpenLog>,
    first_file: bool,
    reported_missing: bool,
}

impl<F: FnMut(WatchMessage)> Watcher<F> {
    fn new(dir: PathBuf, on_message: F) -> Self {
        Self {
            dir,
            on_message,
            log: None,
            first_file: true,
            reported_missing: false,
        }
    }

    fn scan(&mut self) {
        let Some(latest) = find_latest_log(&self.dir) else {
            if !self.reported_missing {
                self.reported_missing = true;
                let dir = self.dir.clone();
                (self.on_message)(WatchMessage::NoLog { dir });
            }
            return;
        };
        if self.log.as_ref().is_some_and(|log| log.path == latest) {
            return;
        }
        // 古いファイルの残りを読み切ってから切り替える
        self.read_new();
        match File::open(&latest) {
            Ok(file) => self.open(latest, file),
            Err(error) => self.report(&error),
        }
    }

    fn open(&mut self, path: PathBuf, file: File) {
        let replay = std::mem::replace(&mut self.first_file, false);
        (self.on_message)(WatchMessage::File {
            path: path.clone(),
            replay,
        });
        self.log = Some(OpenLog {
            path,
            file,
            position: 0,
            pending: Vec::new(),
        });
        if replay {
            self.drain(true);
            (self.on_message)(WatchMessage::ReplayDone);
        }
    }

    fn read_new(&mut self) {
        self.drain(false);
    }

    fn drain(&mut self, replay: bool) {
        if let Err(error) = self.try_drain(replay) {
            self.report(&error);
        }
    }

    fn try_drain(&mut self, replay: bool) -> io::Result<()> {
        let Some(log) = self.log.as_mut() else {
            return Ok(());
        };
        if log.file.metadata()?.len() < log.position {
            log.file.seek(SeekFrom::Start(0))?;
            log.position = 0;
            log.pending.clear();
        }
        let mut chunk = vec![0; READ_CHUNK];
        loop {
            let read = log.file.read(&mut chunk)?;
            if read == 0 {
                return Ok(());
            }
            log.position += read as u64;
            log.pending.extend_from_slice(&chunk[..read]);
            let events = take_events(&mut log.pending);
            if !events.is_empty() {
                (self.on_message)(WatchMessage::Events { events, replay });
            }
        }
    }

    fn report(&mut self, error: &io::Error) {
        (self.on_message)(WatchMessage::Error {
            message: error.to_string(),
        });
    }
}

/// 改行まで届いた行だけを解析し、続きが来ていない行は残しておく。
fn take_events(pending: &mut Vec<u8>) -> Vec<TimedEvent> {
    let Some(end) = pending.iter().rposition(|&byte| byte == b'\n') else {
        return Vec::new();
    };
    let events = pending[..end]
        .split(|&byte| byte == b'\n')
        .filter_map(|line| parse_line(&String::from_utf8_lossy(line)))
        .collect();
    pending.drain(..=end);
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::LogEvent;
    use std::io::Write;
    use std::sync::mpsc::Receiver;

    const FIRST: &str = "output_log_2026-11-03_21-58-12.txt";
    const SECOND: &str = "output_log_2026-11-04_08-00-00.txt";
    const FAST: Timing = Timing {
        poll: Duration::from_millis(10),
        rescan: Duration::from_millis(30),
    };

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(test: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "monitor-core-{test}-{}-{:?}",
                std::process::id(),
                thread::current().id()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn append(&self, name: &str, text: &str) {
            let mut file = File::options()
                .create(true)
                .append(true)
                .open(self.0.join(name))
                .unwrap();
            file.write_all(text.as_bytes()).unwrap();
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn line(clock: &str, message: &str) -> String {
        format!("2026.11.03 {clock} Debug      -  {message}")
    }

    fn start(dir: &TempDir) -> (WatcherHandle, Receiver<WatchMessage>) {
        let (sender, receiver) = mpsc::channel();
        let handle = spawn(dir.0.clone(), FAST, move |message| {
            let _ = sender.send(message);
        });
        (handle, receiver)
    }

    fn next(receiver: &Receiver<WatchMessage>) -> WatchMessage {
        receiver.recv_timeout(Duration::from_secs(5)).unwrap()
    }

    #[test]
    fn replays_tails_keeps_partial_lines_and_switches_files() {
        let dir = TempDir::new("flow");
        let world = line("21:58:22", "[Behaviour] Entering Room: World");
        dir.append(FIRST, &format!("{world}\nignored line\n"));
        let (_handle, receiver) = start(&dir);

        assert!(matches!(
            next(&receiver),
            WatchMessage::File { replay: true, .. }
        ));
        let WatchMessage::Events { events, replay } = next(&receiver) else {
            panic!("expected events");
        };
        assert!(replay);
        assert_eq!(
            events[0].event,
            LogEvent::EnteringRoom {
                world: "World".into()
            }
        );
        assert_eq!(next(&receiver), WatchMessage::ReplayDone);

        // 行の途中までしか書かれていない間はイベントにしない
        let joined = line("22:01:03", "[Behaviour] OnPlayerJoined Bob");
        let (head, tail) = joined.split_at(30);
        dir.append(FIRST, head);
        assert!(receiver.recv_timeout(Duration::from_millis(150)).is_err());
        dir.append(FIRST, &format!("{tail}\r\n"));
        let WatchMessage::Events { events, replay } = next(&receiver) else {
            panic!("expected events");
        };
        assert!(!replay);
        assert_eq!(
            events[0].event,
            LogEvent::PlayerJoined {
                name: "Bob".into(),
                user_id: None
            }
        );

        // 新しいログは起動後のものなので、過去分として扱わない
        dir.append(SECOND, &format!("{world}\n"));
        let WatchMessage::File { path, replay } = next(&receiver) else {
            panic!("expected file");
        };
        assert!(path.ends_with(SECOND));
        assert!(!replay);
        assert!(matches!(
            next(&receiver),
            WatchMessage::Events { replay: false, .. }
        ));
    }

    #[test]
    fn reports_missing_log_once() {
        let dir = TempDir::new("missing");
        let (_handle, receiver) = start(&dir);
        assert_eq!(next(&receiver), WatchMessage::NoLog { dir: dir.0.clone() });
        assert!(receiver.recv_timeout(Duration::from_millis(100)).is_err());
    }

    #[test]
    fn finds_latest_by_name() {
        let dir = TempDir::new("latest");
        dir.append(SECOND, "");
        dir.append(FIRST, "");
        dir.append("unrelated.txt", "");
        let latest = find_latest_log(&dir.0).unwrap();
        assert!(latest.ends_with(SECOND));
    }

    #[test]
    fn missing_directory_has_no_latest() {
        assert_eq!(find_latest_log(Path::new("Z:/does/not/exist")), None);
    }
}
