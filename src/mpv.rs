//! libmpv の薄い FFI ラッパー (libmpv-2.dll を実行時に動的ロードする)
//!
//! 既存の Rust バインディングは更新が止まっており、Windows/MSVC での
//! インポートライブラリ管理が面倒なため、必要な関数だけを自作している。

use crate::error::{Error, Result};
use libloading::Library;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::Path;

/// libmpv のクライアントコンテキスト (`mpv_handle*`)
pub type MpvHandle = *mut c_void;

/// `mpv_wait_event` のタイムアウト 0 はノンブロッキング
const NO_BLOCK: f64 = 0.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
enum EventId {
    None = 0,
    LogMessage = 2,
}

impl EventId {
    fn from_raw(raw: c_int) -> Self {
        match raw {
            2 => Self::LogMessage,
            _ => Self::None,
        }
    }
}

/// `mpv_event` に対応する C 構造体 (使用するフィールドのみ)
#[repr(C)]
struct RawEvent {
    event_id: c_int,
    error: c_int,
    reply_userdata: u64,
    data: *mut c_void,
}

/// `mpv_event_log_message` に対応する C 構造体
#[repr(C)]
struct RawLogMessage {
    prefix: *const c_char,
    level: *const c_char,
    text: *const c_char,
    log_level: c_int,
}

/// ロードした mpv ライブラリ。関数シンボルは呼び出しのたびに解決する。
pub struct MpvLib {
    lib: Library,
}

impl MpvLib {
    /// 指定パスから `libmpv-2.dll` をロードする
    pub fn load(path: &Path) -> Result<Self> {
        unsafe { Library::new(path) }
            .map(|lib| Self { lib })
            .map_err(|e| Error::MpvLibrary(e.to_string()))
    }

    /// シンボルを取得する
    unsafe fn symbol<'a, T: 'static>(&'a self, name: &[u8]) -> Result<libloading::Symbol<'a, T>> {
        unsafe {
            self.lib
                .get(name)
                .map_err(|_| Error::MpvMissingSymbol(String::from_utf8_lossy(name).into_owned()))
        }
    }

    /// mpv の戻り値をチェックし、失敗ならエラー文字列付きで返す
    fn check(&self, code: c_int) -> Result<()> {
        if code == 0 {
            Ok(())
        } else {
            Err(Error::Mpv(self.error_string(code), code))
        }
    }

    /// mpv コンテキストを新規作成する
    pub fn create(&self) -> Result<MpvHandle> {
        unsafe {
            let f: libloading::Symbol<unsafe extern "C" fn() -> MpvHandle> =
                self.symbol(b"mpv_create")?;
            Ok(f())
        }
    }

    /// コンテキストを破棄する
    pub fn destroy(&self, ctx: MpvHandle) {
        unsafe {
            if let Ok(f) = self.symbol::<unsafe extern "C" fn(MpvHandle)>(b"mpv_destroy") {
                f(ctx);
            }
        }
    }

    /// mpv を初期化する
    pub fn initialize(&self, ctx: MpvHandle) -> Result<()> {
        unsafe {
            let f: libloading::Symbol<unsafe extern "C" fn(MpvHandle) -> c_int> =
                self.symbol(b"mpv_initialize")?;
            self.check(f(ctx))
        }
    }

    /// オプションを文字列で設定する (初期化前にのみ有効なものも含む)
    pub fn set_option_string(&self, ctx: MpvHandle, name: &str, value: &str) -> Result<()> {
        let name = cstr(name)?;
        let value = cstr(value)?;
        unsafe {
            let f: libloading::Symbol<
                unsafe extern "C" fn(MpvHandle, *const c_char, *const c_char) -> c_int,
            > = self.symbol(b"mpv_set_option_string")?;
            self.check(f(ctx, name.as_ptr(), value.as_ptr()))
        }
    }

    /// プロパティを文字列で設定する (再生中の変更可)
    pub fn set_property_string(&self, ctx: MpvHandle, name: &str, value: &str) -> Result<()> {
        let name = cstr(name)?;
        let value = cstr(value)?;
        unsafe {
            let f: libloading::Symbol<
                unsafe extern "C" fn(MpvHandle, *const c_char, *const c_char) -> c_int,
            > = self.symbol(b"mpv_set_property_string")?;
            self.check(f(ctx, name.as_ptr(), value.as_ptr()))
        }
    }

    /// プロパティを文字列で取得する (動画サイズなど再生中の状態を読むため)
    pub fn get_property_string(&self, ctx: MpvHandle, prop: &str) -> Result<String> {
        let name = cstr(prop)?;
        unsafe {
            let f: libloading::Symbol<
                unsafe extern "C" fn(MpvHandle, *const c_char) -> *const c_char,
            > = self.symbol(b"mpv_get_property_string")?;
            let ptr = f(ctx, name.as_ptr());
            if ptr.is_null() {
                // プロパティが未確定 (例: 動画読み込み前) の場合はエラー扱い
                return Err(Error::Mpv(format!("プロパティ {prop} を取得できません"), 0));
            }
            let value = CStr::from_ptr(ptr).to_string_lossy().into_owned();
            // 戻り値は mpv 側で確保されたメモリなので mpv_free で解放する
            if let Ok(free) = self.symbol::<unsafe extern "C" fn(*mut c_void)>(b"mpv_free") {
                free(ptr as *mut c_void);
            }
            Ok(value)
        }
    }

    /// mpv コマンドを実行する (args は最後に NULL を付ける)
    pub fn command(&self, ctx: MpvHandle, args: &[&str]) -> Result<()> {
        let owned: Vec<CString> = args.iter().map(|s| cstr(s)).collect::<Result<_>>()?;
        let mut raw: Vec<*const c_char> = owned.iter().map(|s| s.as_ptr()).collect();
        raw.push(std::ptr::null());
        unsafe {
            let f: libloading::Symbol<
                unsafe extern "C" fn(MpvHandle, *const *const c_char) -> c_int,
            > = self.symbol(b"mpv_command")?;
            self.check(f(ctx, raw.as_ptr()))
        }
    }

    /// mpv のログを要求する (level: "error" など)
    pub fn request_log_messages(&self, ctx: MpvHandle, level: &str) -> Result<()> {
        let level = cstr(level)?;
        unsafe {
            let f: libloading::Symbol<unsafe extern "C" fn(MpvHandle, *const c_char) -> c_int> =
                self.symbol(b"mpv_request_log_messages")?;
            self.check(f(ctx, level.as_ptr()))
        }
    }

    /// イベントを 1 つ取得する (timeout=0 でノンブロッキング、無ければ None)
    pub fn wait_event(&self, ctx: MpvHandle) -> Option<RawEventView<'_>> {
        unsafe {
            let f: libloading::Symbol<unsafe extern "C" fn(MpvHandle, f64) -> *const RawEvent> =
                self.symbol(b"mpv_wait_event").ok()?;
            let ptr = f(ctx, NO_BLOCK);
            if ptr.is_null() {
                return None;
            }
            let event = &*ptr;
            if EventId::from_raw(event.event_id) == EventId::None {
                return None;
            }
            Some(RawEventView { event })
        }
    }

    /// エラーコードを文字列化する
    pub fn error_string(&self, code: c_int) -> String {
        unsafe {
            match self.symbol::<unsafe extern "C" fn(c_int) -> *const c_char>(b"mpv_error_string") {
                Ok(f) => {
                    let ptr = f(code);
                    if ptr.is_null() {
                        code.to_string()
                    } else {
                        CStr::from_ptr(ptr).to_string_lossy().into_owned()
                    }
                }
                Err(_) => code.to_string(),
            }
        }
    }
}

/// `mpv_wait_event` の結果 (借用した C イベント)
pub struct RawEventView<'a> {
    event: &'a RawEvent,
}

impl RawEventView<'_> {
    pub fn is_log_message(&self) -> Option<LogMessageView<'_>> {
        if EventId::from_raw(self.event.event_id) == EventId::LogMessage
            && !self.event.data.is_null()
        {
            Some(LogMessageView {
                msg: unsafe { &*(self.event.data as *const RawLogMessage) },
            })
        } else {
            None
        }
    }
}

/// `mpv_event_log_message` の借用ビュー
pub struct LogMessageView<'a> {
    msg: &'a RawLogMessage,
}

impl LogMessageView<'_> {
    pub fn level(&self) -> String {
        cstr_to_string(self.msg.level)
    }

    pub fn text(&self) -> String {
        cstr_to_string(self.msg.text)
    }
}

/// 文字列を C 文字列へ変換する (NUL 混入は不正)
fn cstr(s: &str) -> Result<CString> {
    CString::new(s).map_err(|_| Error::Mpv("文字列に NUL が含まれます".into(), 0))
}

/// ヌル終端の C 文字列を Rust 文字列にする
fn cstr_to_string(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

/// 所有権を持つ mpv クライアントコンテキスト
pub struct Mpv {
    lib: MpvLib,
    ctx: MpvHandle,
}

impl Mpv {
    /// mpv コンテキストを生成する (初期化は行わない)
    pub fn new(lib: MpvLib) -> Result<Self> {
        let ctx = lib.create()?;
        if ctx.is_null() {
            return Err(Error::Mpv("mpv_create が NULL を返しました".into(), 0));
        }
        Ok(Self { lib, ctx })
    }

    pub fn initialize(&mut self) -> Result<()> {
        self.lib.initialize(self.ctx)
    }

    pub fn set_option(&mut self, name: &str, value: &str) -> Result<()> {
        self.lib.set_option_string(self.ctx, name, value)
    }

    pub fn set_property(&mut self, name: &str, value: &str) -> Result<()> {
        self.lib.set_property_string(self.ctx, name, value)
    }

    pub fn get_property(&self, name: &str) -> Result<String> {
        self.lib.get_property_string(self.ctx, name)
    }

    pub fn command(&mut self, args: &[&str]) -> Result<()> {
        self.lib.command(self.ctx, args)
    }

    pub fn request_log_messages(&mut self, level: &str) -> Result<()> {
        self.lib.request_log_messages(self.ctx, level)
    }

    /// キューに溜まったイベントを処理する (主にログ取得用)
    pub fn pump_events(&mut self) {
        while let Some(event) = self.lib.wait_event(self.ctx) {
            if let Some(log) = event.is_log_message() {
                let level = log.level();
                if matches!(level.as_str(), "warn" | "error" | "fatal") {
                    log::warn!("[mpv:{level}] {}", log.text().trim_end());
                }
            }
        }
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        self.lib.destroy(self.ctx);
    }
}
