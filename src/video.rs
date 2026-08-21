//! 壁紙動画の再生を担うプレイヤー (libmpv の設定と操作)

use crate::error::{Error, Result};
use crate::mpv::{Mpv, MpvLib};
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::HWND;

/// ディスプレイの垂直同期を有効にしたまま動画本来のレートで再生するための
/// 基本オプション (詳細は mpv マニュアルの options.rst を参照)。
///
/// - `video-sync` は既定の `audio` のままにする。
///   音声無効時はシステムクロック基準になり、動画は本来のフレームレートで再生される。
/// - `panscan=1.0` で画面一杯にクロップして表示する (cover 相当)。
const BASE_OPTIONS: &[(&str, &str)] = &[
    ("vo", "gpu"),
    ("gpu-api", "d3d11"),
    ("d3d11-flip", "yes"),
    ("hwdec", "auto"),
    ("panscan", "1.0"),
    ("loop-file", "inf"),
    ("idle", "yes"),
    ("osd-level", "0"),
    ("input-default-bindings", "no"),
    ("input-vo-keyboard", "no"),
];

/// 壁紙動画プレイヤー
pub struct VideoPlayer {
    mpv: Mpv,
    muted: bool,
}

impl VideoPlayer {
    /// 指定ウィンドウ (HWND) に埋め込んでプレイヤーを構築する
    pub fn new(window: HWND, muted: bool) -> Result<Self> {
        let lib = MpvLib::load(&find_mpv_dll())?;
        let mut mpv = Mpv::new(lib)?;

        // 初期化前にウィンドウハンドルと基本オプションを設定する
        mpv.set_option("wid", &(window.0 as i64).to_string())?;
        for (name, value) in BASE_OPTIONS {
            mpv.set_option(name, value)?;
        }
        // ミュート時は音声デコード自体を無効化して CPU 負荷を下げる
        if muted {
            mpv.set_option("audio", "no")?;
        }
        mpv.set_option("volume", "100")?;

        mpv.initialize()?;
        mpv.request_log_messages("warn")?;

        Ok(Self { mpv, muted })
    }

    /// 動画を読み込んで再生を開始する
    pub fn load_file(&mut self, path: &Path) -> Result<()> {
        if !path.is_file() {
            return Err(Error::FileNotFound(path.to_path_buf()));
        }
        let arg = path.to_string_lossy();
        self.mpv.command(&["loadfile", &arg])?;
        log::info!("動画を再生します: {arg}");
        Ok(())
    }

    /// ミュート状態を切り替える
    pub fn set_muted(&mut self, muted: bool) -> Result<()> {
        if muted {
            // 音声トラックを外し、ミュートも設定する
            self.mpv.set_property("aid", "no")?;
            self.mpv.set_property("mute", "yes")?;
        } else {
            self.mpv.set_property("aid", "auto")?;
            self.mpv.set_property("mute", "no")?;
        }
        self.muted = muted;
        log::info!(
            "音声を {} にしました",
            if muted { "ミュート" } else { "有効" }
        );
        Ok(())
    }

    /// 現在のミュート状態
    pub fn muted(&self) -> bool {
        self.muted
    }

    /// 溜まっている mpv のイベント (ログ等) を処理する
    pub fn pump_events(&mut self) {
        self.mpv.pump_events();
    }
}

/// libmpv-2.dll の場所を探索する
///
/// 1. 実行ファイルと同じディレクトリ
/// 2. 実行ファイル直下の `libs`
/// 3. カレントディレクトリ
/// 4. カレントディレクトリの `libs`
fn find_mpv_dll() -> PathBuf {
    const CANDIDATES: [&str; 2] = ["libmpv-2.dll", "libs/libmpv-2.dll"];
    let mut list: Vec<PathBuf> = Vec::new();

    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        for name in CANDIDATES {
            list.push(dir.join(name));
        }
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    for name in CANDIDATES {
        list.push(cwd.join(name));
    }

    list.into_iter()
        .find(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("libmpv-2.dll"))
}
