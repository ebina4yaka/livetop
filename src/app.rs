//! アプリ本体の起動・メインループ・各機能の統合
//!
//! 設定ウィンドウは別プロセス (`livetop --settings`) で動かし、
//! 設定ファイル (config.toml) の変更を監視して反映する。

use crate::config::{self, BackgroundFit, Config, DisplayMode};
use crate::wallpaper::{DesktopHost, MonitorInfo, WallpaperWindow};
use crate::{autostart, tray, video, wallpaper};
use std::path::Path;
use std::time::{Duration, Instant};
use tray_icon::{MouseButton, MouseButtonState, TrayIconEvent};

/// WorkerW への再付着を確認する間隔
const ATTACH_CHECK_INTERVAL: Duration = Duration::from_secs(3);
/// メインループのポーリング間隔
const LOOP_SLEEP: Duration = Duration::from_millis(16);
/// 設定ファイルの変更を確認する間隔
const CONFIG_CHECK_INTERVAL: Duration = Duration::from_secs(1);
/// モニタ構成の変更を確認する間隔
const MONITOR_CHECK_INTERVAL: Duration = Duration::from_secs(2);

/// 壁紙ウィンドウ 1 つ分 (ウィンドウ + そのウィンドウ用の動画プレイヤー)
struct WallpaperInstance {
    window: WallpaperWindow,
    player: Option<video::VideoPlayer>,
    /// `monitors()` の順でのディスプレイ番号 (PerDisplay モードで使用)
    display_index: usize,
    /// 現在読み込んでいる動画 (変更検知用)
    current_video: Option<std::path::PathBuf>,
}

/// アプリを起動する。`video_arg` はコマンドライン引数で指定された動画パス。
pub fn run(video_arg: Option<String>) {
    // DPI 認識 (モニタ間で座標がずれないように)
    wallpaper::set_dpi_awareness();

    // 多重起動防止
    if wallpaper::SingleInstance::acquire().is_none() {
        log::info!("既に起動中のため終了します");
        return;
    }

    // 設定の読み込み (CLI 引数があれば反映して保存)
    let mut config = Config::load();
    if let Some(path) = video_arg {
        log::info!("コマンドラインから動画を指定されました: {path}");
        config.video_path = Some(path.into());
        let _ = config.save();
    }

    // 壁紙ウィンドウと動画プレイヤーの初期構築
    let mut instances: Vec<WallpaperInstance> = Vec::new();
    let mut desktop_host = DesktopHost::default();
    rebuild_wallpapers(&config, &mut instances, &mut desktop_host);

    // トレイアイコンの作成
    let tray = match tray::Tray::build(config.autostart) {
        Ok(t) => Some(t),
        Err(e) => {
            log::warn!("トレイアイコンの作成に失敗しました: {e}; 終了はタスクマネージャーから");
            None
        }
    };

    // 設定ファイルの最終変更時刻 (外部から書き換えられたかを監視する)
    let mut last_config_mtime = config_file_mtime();
    let mut last_config_check = Instant::now() - CONFIG_CHECK_INTERVAL;

    // モニタ構成の追跡 (変更されたら壁紙レイアウトを作り直す)
    let mut last_monitors = wallpaper::monitors();
    let mut last_monitor_check = Instant::now() - MONITOR_CHECK_INTERVAL;

    // 初回起動で動画未設定の場合は設定ウィンドウを開く (一度だけ開いてフラグを落とす)
    let mut open_settings_on_start = !config.has_valid_video();

    // メインループ
    let mut quit = false;
    let mut last_attach_check = Instant::now() - ATTACH_CHECK_INTERVAL;

    while !quit {
        wallpaper::pump_messages(&mut quit);

        // トレイのクリック/メニューイベントを処理
        handle_tray_events(
            &tray,
            &mut instances,
            &mut config,
            &mut quit,
            &mut last_config_mtime,
        );

        // 設定ファイルの変更監視 (設定ウィンドウが保存した内容を反映)
        if last_config_check.elapsed() >= CONFIG_CHECK_INTERVAL {
            last_config_check = Instant::now();
            let now_mtime = config_file_mtime();
            if now_mtime != last_config_mtime {
                last_config_mtime = now_mtime;
                let new_config = Config::load();
                apply_config_changes(
                    &config,
                    &new_config,
                    &mut instances,
                    &mut desktop_host,
                    &tray,
                );
                config = new_config;
            }
        }

        // モニタ構成の変更監視 (接続/切断でレイアウトを作り直す)
        if last_monitor_check.elapsed() >= MONITOR_CHECK_INTERVAL {
            last_monitor_check = Instant::now();
            let monitors = wallpaper::monitors();
            if monitors != last_monitors {
                log::info!("モニタ構成が変化したため壁紙レイアウトを作り直します");
                last_monitors = monitors;
                rebuild_wallpapers(&config, &mut instances, &mut desktop_host);
            }
        }

        // 初回の設定ウィンドウ表示
        if open_settings_on_start {
            open_settings_on_start = false;
            open_settings_process();
        }

        // Explorer 再起動などで外れた場合の再付着
        if last_attach_check.elapsed() >= ATTACH_CHECK_INTERVAL {
            for inst in &instances {
                desktop_host.ensure(inst.window.hwnd(), &inst.window.rect);
            }
            last_attach_check = Instant::now();
        }

        // mpv のイベント (ログ等) を処理
        for inst in &mut instances {
            if let Some(p) = inst.player.as_mut() {
                p.pump_events();
            }
        }

        std::thread::sleep(LOOP_SLEEP);
    }

    // 後片付け (mpv を先に破棄してからウィンドウを破棄する)
    for inst in &mut instances {
        inst.player = None;
    }
    log::info!("Livetop を終了します");
}

/// 設定に応じた壁紙ウィンドウ一式を作り直す
///
/// 適用モード:
/// - `Spanning`   : 全モニタを覆う 1 ウィンドウ
/// - `PerDisplay` : 各モニタ毎に 1 ウィンドウ
/// - `Specific`   : 指定モニタのみの 1 ウィンドウ
fn rebuild_wallpapers(
    config: &Config,
    instances: &mut Vec<WallpaperInstance>,
    desktop: &mut DesktopHost,
) {
    let target = target_rects(&config.display_mode, config.display_index);

    // 現在と同じレイアウトなら作り直さない (プレイヤーを維持する)
    let mut current: Vec<MonitorInfo> = instances.iter().map(|i| i.window.rect).collect();
    let mut target_sorted = target.clone();
    current.sort();
    target_sorted.sort();
    if current == target_sorted {
        return;
    }

    log::info!(
        "壁紙レイアウトを適用します (モード={}, ウィンドウ数={})",
        config.display_mode.label(),
        target.len()
    );

    // 既存を破棄 (mpv を先に破棄してからウィンドウを破棄する)
    for inst in instances.iter_mut() {
        inst.player = None;
    }
    instances.clear();

    for (i, rect) in target.iter().enumerate() {
        let window = match WallpaperWindow::create(*rect) {
            Ok(w) => w,
            Err(e) => {
                log::error!("壁紙ウィンドウの作成に失敗しました: {e}");
                continue;
            }
        };
        let hwnd = window.hwnd();
        if !desktop.attach(hwnd, rect) {
            log::warn!("デスクトップレイヤーへの付着に失敗しました");
        }

        // PerDisplay モードでは列挙順をディスプレイ番号として扱う
        let display_index = if config.display_mode == DisplayMode::PerDisplay {
            i
        } else {
            0
        };
        let video = config.video_for_display(display_index);

        // 各ウィンドウに動画プレイヤーを作成する
        let window_size = (rect.width, rect.height);
        let mut player = match video::VideoPlayer::new(
            hwnd,
            window_size,
            config.muted,
            config.background_fit,
        ) {
            Ok(p) => Some(p),
            Err(e) => {
                log::error!(
                    "動画プレイヤーを初期化できませんでした: {e} (libmpv-2.dll を確認してください)"
                );
                None
            }
        };
        if let Some(p) = player.as_mut()
            && let Some(path) = video.as_ref()
            && let Err(e) = p.load_file(path)
        {
            log::error!("動画の読み込みに失敗しました: {e}");
        }

        instances.push(WallpaperInstance {
            window,
            player,
            display_index,
            current_video: video,
        });
    }
}

/// 適用モードに応じた対象領域の一覧を求める
fn target_rects(mode: &DisplayMode, index: usize) -> Vec<MonitorInfo> {
    let monitors = wallpaper::monitors();
    match mode {
        DisplayMode::Spanning => vec![MonitorInfo::virtual_screen()],
        DisplayMode::PerDisplay => monitors,
        DisplayMode::Specific => {
            let idx = index.min(monitors.len().saturating_sub(1));
            match monitors.get(idx) {
                Some(m) => vec![*m],
                // モニタが無ければ全画面にフォールバック
                None => vec![MonitorInfo::virtual_screen()],
            }
        }
    }
}

/// トレイのクリックとメニューイベントを処理する
fn handle_tray_events(
    tray: &Option<tray::Tray>,
    instances: &mut [WallpaperInstance],
    config: &mut Config,
    quit: &mut bool,
    last_config_mtime: &mut u64,
) {
    // トレイアイコンのクリック (左クリックで設定を開く)
    while let Ok(event) = TrayIconEvent::receiver().try_recv() {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            open_settings_process();
        }
    }

    let Some(tray) = tray else { return };

    // メニューのクリック
    while let Ok(event) = muda::MenuEvent::receiver().try_recv() {
        if event.id == tray.menu.settings_id {
            open_settings_process();
        } else if event.id == tray.menu.change_video_id {
            pick_and_apply_video(instances, config, last_config_mtime);
        } else if event.id == tray.menu.autostart_id {
            toggle_autostart(config, tray, last_config_mtime);
        } else if event.id == tray.menu.quit_id {
            log::info!("トレイメニューから終了します");
            *quit = true;
        }
    }
}

/// 設定ウィンドウを別プロセス (`livetop --settings`) で開く
fn open_settings_process() {
    let exe = std::env::current_exe().expect("実行ファイルのパスを取得できません");
    match std::process::Command::new(exe).arg("--settings").spawn() {
        Ok(_) => log::info!("設定ウィンドウを起動しました"),
        Err(e) => log::error!("設定ウィンドウの起動に失敗しました: {e}"),
    }
}

/// ファイル選択ダイアログで動画を選んで既定の壁紙動画として適用する
fn pick_and_apply_video(
    instances: &mut [WallpaperInstance],
    config: &mut Config,
    last_config_mtime: &mut u64,
) {
    if let Some(path) = crate::dialog::pick_video_file() {
        config.video_path = Some(path);
        if let Err(e) = config.save() {
            log::error!("設定の保存に失敗しました: {e}");
        }
        // 全ウィンドウへ適用 (PerDisplay で個別指定のあるディスプレイは維持される)
        apply_videos_to_all(instances, config);
        *last_config_mtime = config_file_mtime();
    }
}

/// 設定の差分を反映する (外部の設定ファイル変更や設定ウィンドウの適用時)
fn apply_config_changes(
    old: &Config,
    new: &Config,
    instances: &mut Vec<WallpaperInstance>,
    desktop: &mut DesktopHost,
    tray: &Option<tray::Tray>,
) {
    // 表示モードや対象ディスプレイの変更があればレイアウトを作り直す
    if old.display_mode != new.display_mode || old.display_index != new.display_index {
        rebuild_wallpapers(new, instances, desktop);
    }

    // 動画 (既定 or ディスプレイ毎) の変更があれば差分を反映する
    if old.video_path != new.video_path || old.display_videos != new.display_videos {
        apply_videos_to_all(instances, new);
    }

    // 背景の合わせ方の変更を全ウィンドウへ反映
    if old.background_fit != new.background_fit {
        set_fit_all(instances, new.background_fit);
    }

    // ミュート設定の変更を全ウィンドウへ反映
    if old.muted != new.muted {
        set_muted_all(instances, new.muted);
    }

    // 自動起動の変更
    if old.autostart != new.autostart
        && let Err(e) = autostart::set_autostart(new.autostart)
    {
        log::error!("自動起動の設定に失敗しました: {e}");
    }
    if let Some(t) = tray {
        t.menu.set_autostart_label(new.autostart);
    }
}

/// 各ウィンドウの動画を設定に合わせて更新する (変更があったものだけ読み込む)
fn apply_videos_to_all(instances: &mut [WallpaperInstance], config: &Config) {
    for inst in instances.iter_mut() {
        let video = config.video_for_display(inst.display_index);
        if video != inst.current_video {
            if let Some(path) = video.as_ref() {
                load_or_recreate_player(inst, path, config.muted, config.background_fit);
            }
            inst.current_video = video;
        }
    }
}

/// 1 ウィンドウ分の動画を読み込む (プレイヤー未初期化なら再作成を試みる)
fn load_or_recreate_player(
    inst: &mut WallpaperInstance,
    path: &Path,
    muted: bool,
    fit: BackgroundFit,
) {
    if let Some(p) = inst.player.as_mut() {
        if let Err(e) = p.load_file(path) {
            log::error!("動画の読み込みに失敗しました: {e}");
        }
    } else {
        let hwnd = inst.window.hwnd();
        let window_size = (inst.window.rect.width, inst.window.rect.height);
        match video::VideoPlayer::new(hwnd, window_size, muted, fit) {
            Ok(mut p) => {
                if let Err(e) = p.load_file(path) {
                    log::error!("動画の読み込みに失敗しました: {e}");
                }
                inst.player = Some(p);
            }
            Err(e) => log::error!("動画プレイヤーの再構築に失敗しました: {e}"),
        }
    }
}

/// 全ウィンドウのミュート状態を切り替える
fn set_muted_all(instances: &mut [WallpaperInstance], muted: bool) {
    for inst in instances.iter_mut() {
        if let Some(p) = inst.player.as_mut()
            && p.muted() != muted
            && let Err(e) = p.set_muted(muted)
        {
            log::error!("音声設定の反映に失敗しました: {e}");
        }
    }
}

/// 全ウィンドウへ背景の合わせ方を適用する
fn set_fit_all(instances: &mut [WallpaperInstance], fit: BackgroundFit) {
    for inst in instances.iter_mut() {
        if let Some(p) = inst.player.as_mut()
            && let Err(e) = p.set_fit(fit)
        {
            log::error!("背景の表示方法の反映に失敗しました: {e}");
        }
    }
}

/// 自動起動の ON/OFF を切り替える
fn toggle_autostart(config: &mut Config, tray: &tray::Tray, last_config_mtime: &mut u64) {
    config.autostart = !config.autostart;
    if let Err(e) = autostart::set_autostart(config.autostart) {
        log::error!("自動起動の設定に失敗しました: {e}");
    }
    tray.menu.set_autostart_label(config.autostart);
    if let Err(e) = config.save() {
        log::error!("設定の保存に失敗しました: {e}");
    }
    *last_config_mtime = config_file_mtime();
}

/// 設定ファイルの最終変更時刻 (ミリ秒) を返す。無ければ 0。
fn config_file_mtime() -> u64 {
    std::fs::metadata(config::config_path())
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
