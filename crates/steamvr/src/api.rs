//! OpenVR の C API の呼び出し。`unsafe` はこのファイルに閉じ込める。
//!
//! OpenVR の関数は DLL の関数表（FnTable）経由で呼ぶ。表の番号とインターフェースの版は、
//! 同梱する DLL と同じ版の `headers/openvr_capi.h` に合わせている。

use libloading::Library;
use std::ffi::{CStr, CString, c_char, c_void};
use std::fmt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const OPENVR_VERSION: &str = "2.15.6";
/// 1 つの exe で配布できるよう DLL を埋め込み、必要になったときにデータフォルダへ展開する
static LIBRARY: &[u8] = include_bytes!("../vendor/openvr_api.dll");

const SYSTEM_TABLE: &CStr = c"FnTable:IVRSystem_026";
const APPLICATIONS_TABLE: &CStr = c"FnTable:IVRApplications_008";

// VR_IVRSystem_FnTable の番号
const POLL_NEXT_EVENT: usize = 30;
const ACKNOWLEDGE_QUIT_EXITING: usize = 47;
// VR_IVRApplications_FnTable の番号
const ADD_APPLICATION_MANIFEST: usize = 0;
const REMOVE_APPLICATION_MANIFEST: usize = 1;
const IS_APPLICATION_INSTALLED: usize = 2;
const APPLICATION_ERROR_NAME: usize = 13;
const SET_APPLICATION_AUTO_LAUNCH: usize = 17;
const GET_APPLICATION_AUTO_LAUNCH: usize = 18;

const INIT_ERROR_NONE: i32 = 0;
const INIT_ERROR_INSTALLATION_NOT_FOUND: i32 = 100;
const INIT_ERROR_NO_SERVER_FOR_BACKGROUND_APP: i32 = 121;
const VREVENT_QUIT: u32 = 700;

type InitInternal2 = unsafe extern "C" fn(*mut i32, i32, *const c_char) -> isize;
type ShutdownInternal = unsafe extern "C" fn();
type GetGenericInterface = unsafe extern "C" fn(*const c_char, *mut i32) -> isize;
type IsRuntimeInstalled = unsafe extern "C" fn() -> u8;
type DescribeInitError = unsafe extern "C" fn(i32) -> *const c_char;

type PollNextEvent = unsafe extern "system" fn(*mut VrEvent, u32) -> u8;
type AcknowledgeQuitExiting = unsafe extern "system" fn();
type ManifestCall = unsafe extern "system" fn(*const c_char, u8) -> i32;
type ManifestPathCall = unsafe extern "system" fn(*const c_char) -> i32;
type AppKeyQuery = unsafe extern "system" fn(*const c_char) -> u8;
type ErrorName = unsafe extern "system" fn(i32) -> *const c_char;

/// `VREvent_t` と同じ配置（Windows では 8 バイト境界）。使うのは先頭の種類だけ。
#[repr(C, align(8))]
struct VrEvent {
    event_type: u32,
    _rest: [u8; 60],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// SteamVR がインストールされていない
    NotInstalled,
    /// SteamVR が起動していない
    NotRunning,
    /// 同梱の DLL を読み込めない
    Library(String),
    /// SteamVR がエラーを返した
    Runtime(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInstalled => f.write_str("SteamVR が見つかりません"),
            Self::NotRunning => f.write_str("SteamVR が起動していません"),
            Self::Library(detail) => write!(f, "OpenVR のライブラリを読み込めません: {detail}"),
            Self::Runtime(detail) => write!(f, "SteamVR のエラー: {detail}"),
        }
    }
}

impl std::error::Error for Error {}

/// 同梱の DLL を読み込めるかだけを確かめる（ビルドした exe の動作確認用）。
pub fn self_check(data_dir: &Path) -> Result<(), Error> {
    let api = OpenVr::load(data_dir)?;
    let _ = api.is_runtime_installed();
    Ok(())
}

fn extract_library(data_dir: &Path) -> std::io::Result<PathBuf> {
    let path = data_dir.join(format!("openvr_api-{OPENVR_VERSION}.dll"));
    let up_to_date = std::fs::metadata(&path).is_ok_and(|m| m.len() == LIBRARY.len() as u64);
    if !up_to_date {
        std::fs::create_dir_all(data_dir)?;
        let temporary = path.with_extension("tmp");
        std::fs::write(&temporary, LIBRARY)?;
        std::fs::rename(&temporary, &path)?;
    }
    Ok(path)
}

#[derive(Debug, Clone, Copy)]
pub enum ConnectionKind {
    /// SteamVR が起動中のときだけ接続する。接続しても SteamVR を起動しない
    Background = 3,
    /// 設定の変更などの短い処理用。SteamVR が止まっていても使える
    Utility = 4,
}

pub struct OpenVr {
    _library: Library,
    init: InitInternal2,
    shutdown: ShutdownInternal,
    get_interface: GetGenericInterface,
    runtime_installed: IsRuntimeInstalled,
    describe_init_error: DescribeInitError,
}

impl OpenVr {
    pub fn load(data_dir: &Path) -> Result<Self, Error> {
        let path = extract_library(data_dir).map_err(|e| Error::Library(e.to_string()))?;
        let library_error = |e: libloading::Error| Error::Library(e.to_string());
        // SAFETY: 読み込むのは同梱の OpenVR の DLL で、関数の型は openvr_capi.h に合わせている。
        // 関数ポインタの有効期間は、同じ構造体が持つ `_library` が保証する。
        unsafe {
            let library = Library::new(&path).map_err(library_error)?;
            let init = *library
                .get::<InitInternal2>(b"VR_InitInternal2\0")
                .map_err(library_error)?;
            let shutdown = *library
                .get::<ShutdownInternal>(b"VR_ShutdownInternal\0")
                .map_err(library_error)?;
            let get_interface = *library
                .get::<GetGenericInterface>(b"VR_GetGenericInterface\0")
                .map_err(library_error)?;
            let runtime_installed = *library
                .get::<IsRuntimeInstalled>(b"VR_IsRuntimeInstalled\0")
                .map_err(library_error)?;
            let describe_init_error = *library
                .get::<DescribeInitError>(b"VR_GetVRInitErrorAsEnglishDescription\0")
                .map_err(library_error)?;
            Ok(Self {
                _library: library,
                init,
                shutdown,
                get_interface,
                runtime_installed,
                describe_init_error,
            })
        }
    }

    pub fn is_runtime_installed(&self) -> bool {
        // SAFETY: 引数の無い問い合わせで、いつ呼んでもよい関数。
        unsafe { (self.runtime_installed)() != 0 }
    }

    pub fn connect(self: &Rc<Self>, kind: ConnectionKind) -> Result<Session, Error> {
        let mut error = INIT_ERROR_NONE;
        // SAFETY: error は呼び出しの間有効で、起動情報は省略できる。
        unsafe { (self.init)(&mut error, kind as i32, std::ptr::null()) };
        match error {
            INIT_ERROR_NONE => {}
            INIT_ERROR_INSTALLATION_NOT_FOUND => return Err(Error::NotInstalled),
            INIT_ERROR_NO_SERVER_FOR_BACKGROUND_APP => return Err(Error::NotRunning),
            code => return Err(Error::Runtime(self.describe(code))),
        }
        // ここから先は Session の破棄で接続が閉じる
        let mut session = Session {
            api: Rc::clone(self),
            system: Table(std::ptr::null()),
            applications: Table(std::ptr::null()),
        };
        session.system = self.table(SYSTEM_TABLE)?;
        session.applications = self.table(APPLICATIONS_TABLE)?;
        Ok(session)
    }

    fn table(&self, name: &CStr) -> Result<Table, Error> {
        let mut error = INIT_ERROR_NONE;
        // SAFETY: name は NUL 終端で、error は呼び出しの間有効。
        let pointer = unsafe { (self.get_interface)(name.as_ptr(), &mut error) };
        if error != INIT_ERROR_NONE || pointer == 0 {
            return Err(Error::Runtime(format!(
                "{}: {}",
                name.to_string_lossy(),
                self.describe(error)
            )));
        }
        Ok(Table(pointer as *const *const c_void))
    }

    fn describe(&self, code: i32) -> String {
        // SAFETY: 戻り値は DLL が持つ静的な文字列。
        unsafe {
            let text = (self.describe_init_error)(code);
            if text.is_null() {
                return format!("error {code}");
            }
            CStr::from_ptr(text).to_string_lossy().into_owned()
        }
    }
}

#[derive(Clone, Copy)]
struct Table(*const *const c_void);

impl Table {
    /// 表の `index` 番目の関数を取り出す。
    ///
    /// # Safety
    /// `F` は、その番号の関数の型と一致していなければならない。
    unsafe fn function<F: Copy>(self, index: usize) -> F {
        assert_eq!(size_of::<F>(), size_of::<*const c_void>());
        // SAFETY: 表は接続中は有効で、番号はヘッダーの定義内にある。型の一致は呼び出し側の責任。
        unsafe { std::mem::transmute_copy(&*self.0.add(index)) }
    }
}

/// SteamVR との接続。破棄すると接続を閉じる。同時に 1 つだけ持てる。
pub struct Session {
    api: Rc<OpenVr>,
    system: Table,
    applications: Table,
}

impl Session {
    /// 届いたイベントを読み進め、SteamVR の終了要求があれば true を返す。
    pub fn poll_quit(&self) -> bool {
        // SAFETY: 型はヘッダーの PollNextEvent に一致する。
        let poll: PollNextEvent = unsafe { self.system.function(POLL_NEXT_EVENT) };
        let mut event = VrEvent {
            event_type: 0,
            _rest: [0; 60],
        };
        // SAFETY: event は書き込み可能で、渡す大きさは実際の大きさと一致する。
        while unsafe { poll(&mut event, size_of::<VrEvent>() as u32) } != 0 {
            if event.event_type == VREVENT_QUIT {
                return true;
            }
        }
        false
    }

    /// 終了要求を受け取ったことを SteamVR に伝える。
    pub fn acknowledge_quit(&self) {
        // SAFETY: 型はヘッダーの AcknowledgeQuit_Exiting に一致する。
        unsafe {
            let acknowledge: AcknowledgeQuitExiting =
                self.system.function(ACKNOWLEDGE_QUIT_EXITING);
            acknowledge();
        }
    }

    pub fn add_manifest(&self, path: &str) -> Result<(), Error> {
        let path = c_string(path)?;
        // SAFETY: 型はヘッダーの AddApplicationManifest に一致し、path は呼び出しの間有効。
        let code = unsafe {
            let add: ManifestCall = self.applications.function(ADD_APPLICATION_MANIFEST);
            add(path.as_ptr(), 0)
        };
        self.check(code)
    }

    pub fn remove_manifest(&self, path: &str) -> Result<(), Error> {
        let path = c_string(path)?;
        // SAFETY: 型はヘッダーの RemoveApplicationManifest に一致し、path は呼び出しの間有効。
        let code = unsafe {
            let remove: ManifestPathCall = self.applications.function(REMOVE_APPLICATION_MANIFEST);
            remove(path.as_ptr())
        };
        self.check(code)
    }

    pub fn is_installed(&self, app_key: &str) -> bool {
        self.query(IS_APPLICATION_INSTALLED, app_key)
    }

    pub fn auto_launch(&self, app_key: &str) -> bool {
        self.query(GET_APPLICATION_AUTO_LAUNCH, app_key)
    }

    pub fn set_auto_launch(&self, app_key: &str, enabled: bool) -> Result<(), Error> {
        let key = c_string(app_key)?;
        // SAFETY: 型はヘッダーの SetApplicationAutoLaunch に一致し、key は呼び出しの間有効。
        let code = unsafe {
            let set: ManifestCall = self.applications.function(SET_APPLICATION_AUTO_LAUNCH);
            set(key.as_ptr(), u8::from(enabled))
        };
        self.check(code)
    }

    fn query(&self, index: usize, app_key: &str) -> bool {
        let Ok(key) = CString::new(app_key) else {
            return false;
        };
        // SAFETY: index はアプリキーを受け取り真偽値を返す関数の番号で、key は呼び出しの間有効。
        unsafe {
            let query: AppKeyQuery = self.applications.function(index);
            query(key.as_ptr()) != 0
        }
    }

    fn check(&self, code: i32) -> Result<(), Error> {
        if code == 0 {
            return Ok(());
        }
        // SAFETY: 型はヘッダーの GetApplicationsErrorNameFromEnum に一致し、
        // 戻り値は DLL が持つ静的な文字列。
        let name = unsafe {
            let name: ErrorName = self.applications.function(APPLICATION_ERROR_NAME);
            let text = name(code);
            if text.is_null() {
                format!("error {code}")
            } else {
                CStr::from_ptr(text).to_string_lossy().into_owned()
            }
        };
        Err(Error::Runtime(name))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: この接続で取得した関数表は、以降使われない。
        unsafe { (self.api.shutdown)() };
    }
}

fn c_string(text: &str) -> Result<CString, Error> {
    CString::new(text).map_err(|e| Error::Runtime(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_matches_openvr_layout() {
        assert_eq!(size_of::<VrEvent>(), 64);
        assert_eq!(align_of::<VrEvent>(), 8);
    }

    #[test]
    fn bundled_library_loads_without_steamvr_running() {
        let dir = std::env::temp_dir().join(format!("steamvr-api-{}", std::process::id()));
        self_check(&dir).unwrap();
        let path = extract_library(&dir).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().len(),
            LIBRARY.len() as u64
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn errors_are_readable() {
        assert_eq!(Error::NotInstalled.to_string(), "SteamVR が見つかりません");
        assert!(
            Error::Runtime("InvalidManifest".into())
                .to_string()
                .contains("InvalidManifest")
        );
    }
}
