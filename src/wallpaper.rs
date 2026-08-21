//! ウィンドウをデスクトップ壁紙レイヤーへ埋め込む処理
//!
//! Windows の `WorkerW` 技法を使い、アイコンやタスクバーより下、
//! 通常の壁紙画像より上のレイヤーにウィンドウを配置する。

use crate::error::{Error, Result};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT,
    WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EnumWindows, FindWindowExW,
    FindWindowW, GW_CHILD, GW_HWNDNEXT, GetClassNameW, GetSystemMetrics, GetWindow, GetWindowRect,
    HWND_BOTTOM, IsWindow, IsWindowVisible, MONITORINFOF_PRIMARY, MSG, PM_REMOVE, PeekMessageW,
    RegisterClassExW, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SMTO_ABORTIFHUNG, SW_SHOWNOACTIVATE,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SendMessageTimeoutW, SetParent, SetWindowPos, ShowWindow,
    TranslateMessage, WM_QUIT, WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::{BOOL, PCWSTR, w};

/// アイコンより下へウィンドウを置くための Progman の内部メッセージ
const PROGMAN_WALLPAPER_MESSAGE: u32 = 0x052C;

const WINDOW_CLASS: PCWSTR = w!("LivetopWallpaper");
const WINDOW_NAME: PCWSTR = w!("Livetop");

/// 1 つのディスプレイの表示領域 (仮想スクリーン座標)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MonitorInfo {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

impl MonitorInfo {
    /// 全モニタを覆う仮想スクリーンの矩形
    pub fn virtual_screen() -> Self {
        let ms = monitors();
        let left = ms.iter().map(|m| m.left).min().unwrap_or(0);
        let top = ms.iter().map(|m| m.top).min().unwrap_or(0);
        let right = ms.iter().map(|m| m.left + m.width).max().unwrap_or(0);
        let bottom = ms.iter().map(|m| m.top + m.height).max().unwrap_or(0);
        Self {
            left,
            top,
            width: right - left,
            height: bottom - top,
        }
    }
}

/// 全モニタの表示領域を列挙する (EnumDisplayMonitors の順)
pub fn monitors() -> Vec<MonitorInfo> {
    unsafe {
        let mut list: Vec<MonitorInfo> = Vec::new();
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(monitor_enum),
            LPARAM(&mut list as *mut _ as isize),
        );
        if list.is_empty() {
            // 列挙できなかった場合の安全策
            list.push(MonitorInfo {
                left: 0,
                top: 0,
                width: GetSystemMetrics(SM_CXVIRTUALSCREEN),
                height: GetSystemMetrics(SM_CYVIRTUALSCREEN),
            });
        }
        list
    }
}

/// `EnumDisplayMonitors` のコールバック (各モニタの矩形を収集する)
unsafe extern "system" fn monitor_enum(
    _monitor: HMONITOR,
    _hdc: HDC,
    rect: *mut RECT,
    data: LPARAM,
) -> BOOL {
    unsafe {
        if !rect.is_null() {
            let list = &mut *(data.0 as *mut Vec<MonitorInfo>);
            list.push(MonitorInfo {
                left: (*rect).left,
                top: (*rect).top,
                width: (*rect).right - (*rect).left,
                height: (*rect).bottom - (*rect).top,
            });
        }
    }
    BOOL(1)
}

/// プライマリ (メイン) ディスプレイの `monitors()` 上のインデックスを返す
///
/// `EnumDisplayMonitors` の列挙順は DPI 設定に依存しないため、
/// このインデックスはどのプロセスからでも同じ値になる。
pub fn primary_display_index() -> usize {
    let mut state = PrimaryScan::default();
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(primary_scan_enum),
            LPARAM(&mut state as *mut _ as isize),
        );
    }
    state.primary.unwrap_or(0)
}

/// `primary_display_index` の走査状態
#[derive(Default)]
struct PrimaryScan {
    index: usize,
    primary: Option<usize>,
}

/// `EnumDisplayMonitors` のコールバック (プライマリモニタのインデックスを探す)
unsafe extern "system" fn primary_scan_enum(
    monitor: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    data: LPARAM,
) -> BOOL {
    unsafe {
        let state = &mut *(data.0 as *mut PrimaryScan);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut info).as_bool()
            && (info.dwFlags & MONITORINFOF_PRIMARY) != 0
        {
            state.primary = Some(state.index);
        }
        state.index += 1;
    }
    BOOL(1)
}

/// 壁紙ウィンドウの WndProc (特別な処理はしない)
unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// デスクトップ壁紙用の境界なしウィンドウ
///
/// Drop 時にウィンドウを破棄する。
pub struct WallpaperWindow {
    hwnd: HWND,
    pub rect: MonitorInfo,
}

impl WallpaperWindow {
    /// 指定領域を覆う壁紙ウィンドウを作成して表示する
    pub fn create(rect: MonitorInfo) -> Result<Self> {
        unsafe {
            let module = GetModuleHandleW(None)
                .map_err(|_| Error::Tray("hInstance の取得に失敗しました".into()))?;
            // HMODULE と HINSTANCE は同じハンドル値なので変換する
            let instance = HINSTANCE(module.0);

            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wnd_proc),
                hInstance: instance,
                lpszClassName: WINDOW_CLASS,
                ..Default::default()
            };
            RegisterClassExW(&class);

            let hwnd = CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                WINDOW_CLASS,
                WINDOW_NAME,
                WS_POPUP,
                rect.left,
                rect.top,
                rect.width,
                rect.height,
                None,
                None,
                Some(instance),
                None,
            )
            .map_err(|_| Error::Tray("壁紙ウィンドウの作成に失敗しました".into()))?;

            if hwnd.is_invalid() {
                return Err(Error::Tray("壁紙ウィンドウの作成に失敗しました".into()));
            }
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            Ok(Self { hwnd, rect })
        }
    }

    /// ウィンドウハンドル
    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }
}

impl Drop for WallpaperWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

/// デスクトップ壁紙レイヤーへの付着を管理する
///
/// `GetParent` は windows-rs の実装上「親が無い」をエラーとして返すため、
/// 付着先のハンドルを自分で保持し、`IsWindow` で破棄を検出して再付着する。
#[derive(Default)]
pub struct DesktopHost {
    parent: Option<HWND>,
}

impl DesktopHost {
    /// 壁紙レイヤーへ付着させる (WorkerW 優先、無ければ Progman 直下)
    pub fn attach(&mut self, hwnd: HWND, rect: &MonitorInfo) -> bool {
        unsafe {
            // 戦略A: 壁紙レイヤーの WorkerW があればそこへ付着する
            if let Some(workerw) = find_workerw() {
                let _ = SetParent(hwnd, Some(workerw));
                self.parent = Some(workerw);
                place_bottom(hwnd, self.parent, rect);
                log::info!(
                    "壁紙ウィンドウを WorkerW (0x{:X}) へ付着させました",
                    workerw.0 as usize
                );
                return true;
            }
            // 戦略B: WorkerW が無い環境では Progman 直下の最背面へ付着する
            if let Some(progman) = find_progman() {
                let _ = SetParent(hwnd, Some(progman));
                self.parent = Some(progman);
                place_bottom(hwnd, self.parent, rect);
                log::info!(
                    "壁紙ウィンドウを Progman (0x{:X}) 直下へ付着させました",
                    progman.0 as usize
                );
                return true;
            }
            log::warn!("WorkerW も Progman も見つかりませんでした");
            self.parent = None;
            false
        }
    }

    /// 付着が維持されているか確認する。
    /// 付着先ウィンドウが破棄された場合 (例: Explorer 再起動) のみ再付着する。
    pub fn ensure(&mut self, hwnd: HWND, rect: &MonitorInfo) -> bool {
        unsafe {
            let parent_gone = self.parent.is_none_or(|p| !IsWindow(Some(p)).as_bool());
            if parent_gone {
                return self.attach(hwnd, rect);
            }
            place_bottom(hwnd, self.parent, rect);
            true
        }
    }
}

/// 指定領域を覆うように最背面へ配置する
///
/// 親ウィンドウのクライアント原点基準で座標を計算するため、
/// 親の座標系が仮想スクリーンとずれていても正しく配置される。
fn place_bottom(hwnd: HWND, parent: Option<HWND>, rect: &MonitorInfo) {
    unsafe {
        let (x, y) = if let Some(parent) = parent {
            let mut pt = POINT::default();
            let _ = ClientToScreen(parent, &mut pt);
            (rect.left - pt.x, rect.top - pt.y)
        } else {
            (rect.left, rect.top)
        };
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_BOTTOM),
            x,
            y,
            rect.width,
            rect.height,
            SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
}

/// デスクトップアイコンの下に置くための `WorkerW` ウィンドウを探す
///
/// 1. `Progman` の子で「可視・全画面」の `WorkerW` (実際の壁紙レイヤー)
/// 2. `SHELLDLL_DefView` を直接保持するトップレベルウィンドウの直後の `WorkerW`
/// 3. 子ウィンドウを持たないトップレベル `WorkerW` (最終手段)
///
/// トップレベルの `WorkerW` はシェル状態によって非表示・小サイズのことがあり、
/// そこへ付着すると動画が見えなくなるため、可視の壁紙レイヤーを優先する。
fn find_workerw() -> Option<HWND> {
    unsafe {
        let progman = find_progman()?;

        // Explorer に壁紙レイヤー用の WorkerW を生成させる
        let mut result = 0usize;
        let _ = SendMessageTimeoutW(
            progman,
            PROGMAN_WALLPAPER_MESSAGE,
            WPARAM(0),
            LPARAM(0),
            SMTO_ABORTIFHUNG,
            1000,
            Some(&mut result),
        );

        // 戦略1: Progman の子で可視・全画面の WorkerW
        if let Some(workerw) = find_visible_fullscreen_workerw(progman) {
            return Some(workerw);
        }

        // 戦略2: SHELLDLL_DefView の直後にある WorkerW
        let mut workerw: Option<HWND> = None;
        let _ = EnumWindows(Some(workerw_enum), LPARAM(&mut workerw as *mut _ as isize));
        if workerw.is_some() {
            return workerw;
        }

        // 戦略3: 子ウィンドウを持たない WorkerW を壁紙レイヤー候補とする
        //        (EnumWindows は前面→背面の順なので、最後に残るのが最背面の候補)
        let mut candidate: Option<HWND> = None;
        let _ = EnumWindows(
            Some(empty_workerw_enum),
            LPARAM(&mut candidate as *mut _ as isize),
        );
        candidate
    }
}

/// ウィンドウのクラス名が `WorkerW` ("WorkerW", 7 文字) かどうか
fn is_workerw_class(hwnd: HWND) -> bool {
    unsafe {
        let mut class = [0u16; 16];
        let len = GetClassNameW(hwnd, &mut class) as usize;
        len == 7 && class[..7] == *w!("WorkerW").as_wide()
    }
}

/// 指定ウィンドウの直下にある「可視・全画面」の `WorkerW` を探す
fn find_visible_fullscreen_workerw(parent: HWND) -> Option<HWND> {
    let vs = MonitorInfo::virtual_screen();
    unsafe {
        let mut child = GetWindow(parent, GW_CHILD).ok().filter(|h| !h.is_invalid());
        let mut first_workerw: Option<HWND> = None;
        while let Some(h) = child {
            if is_workerw_class(h) {
                if first_workerw.is_none() {
                    first_workerw = Some(h);
                }
                let visible = IsWindow(Some(h)).as_bool() && IsWindowVisible(h).as_bool();
                let mut r = RECT::default();
                let fullscreen = GetWindowRect(h, &mut r).is_ok()
                    && (r.right - r.left) >= vs.width
                    && (r.bottom - r.top) >= vs.height;
                if visible && fullscreen {
                    return Some(h);
                }
            }
            child = GetWindow(h, GW_HWNDNEXT).ok().filter(|h| !h.is_invalid());
        }
        first_workerw
    }
}

/// `Progman` (デスクトップ) ウィンドウを探す
fn find_progman() -> Option<HWND> {
    unsafe {
        let progman = FindWindowW(w!("Progman"), PCWSTR::null()).ok()?;
        if progman.is_invalid() {
            None
        } else {
            Some(progman)
        }
    }
}

/// `EnumWindows` のコールバック: 子ウィンドウを持たない `WorkerW` を探す
unsafe extern "system" fn empty_workerw_enum(hwnd: HWND, lparam: LPARAM) -> BOOL {
    unsafe {
        if is_workerw_class(hwnd) {
            let child = GetWindow(hwnd, GW_CHILD).ok();
            if child.is_none_or(|h| h.is_invalid()) {
                let slot = &mut *(lparam.0 as *mut Option<HWND>);
                *slot = Some(hwnd);
            }
        }
        BOOL(1)
    }
}

/// `EnumWindows` のコールバック: `SHELLDLL_DefView` を持つウィンドウの
/// 直後に並ぶ `WorkerW` (アイコンより下のレイヤー) を探す
unsafe extern "system" fn workerw_enum(hwnd: HWND, lparam: LPARAM) -> BOOL {
    unsafe {
        let defview = FindWindowExW(Some(hwnd), None, w!("SHELLDLL_DefView"), PCWSTR::null()).ok();
        if let Some(defview) = defview
            && !defview.is_invalid()
        {
            let worker = FindWindowExW(None, Some(hwnd), w!("WorkerW"), PCWSTR::null()).ok();
            if let Some(worker) = worker
                && !worker.is_invalid()
            {
                let slot = &mut *(lparam.0 as *mut Option<HWND>);
                *slot = Some(worker);
                return BOOL(0);
            }
        }
        BOOL(1)
    }
}

/// 多重起動を防ぐ名前付きミューテックス
pub struct SingleInstance {
    handle: windows::Win32::Foundation::HANDLE,
}

impl SingleInstance {
    /// ミューテックスを取得する。既に起動済みなら `None`。
    pub fn acquire() -> Option<Self> {
        unsafe {
            let handle = CreateMutexW(None, false, w!("Local\\LivetopSingleInstance")).ok()?;
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let _ = CloseHandle(handle);
                None
            } else {
                Some(Self { handle })
            }
        }
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

/// DPI 認識を per-monitor v2 に設定する (モニタ間で大きさが揃うように)
pub fn set_dpi_awareness() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

/// キューに溜まった Windows メッセージを処理する。WM_QUIT を受け取ったら `quit` を true にする。
pub fn pump_messages(quit: &mut bool) {
    unsafe {
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            if msg.message == WM_QUIT {
                *quit = true;
            } else {
                let _ = TranslateMessage(&msg);
                let _ = DispatchMessageW(&msg);
            }
        }
    }
}
