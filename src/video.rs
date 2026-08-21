//! 壁紙動画の再生を担うプレイヤー (libmpv の設定と操作)

use crate::config::BackgroundFit;
use crate::error::{Error, Result};
use crate::mpv::{Mpv, MpvLib};
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::HWND;

/// ディスプレイの垂直同期を有効にしたまま動画本来のレートで再生するための
/// 基本オプション (詳細は mpv マニュアルの options.rst を参照)。
///
/// - `video-sync` は既定の `audio` のままにする。
///   音声無効時はシステムクロック基準になり、動画は本来のフレームレートで再生される。
/// - 表示方法 (`panscan` / `video-unscaled` / `video-zoom`) は
///   `BackgroundFit` ごとに `apply_fit` で設定するためここには含めない。
const BASE_OPTIONS: &[(&str, &str)] = &[
    ("vo", "gpu"),
    ("gpu-api", "d3d11"),
    ("d3d11-flip", "yes"),
    ("hwdec", "auto"),
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
    /// 現在適用している背景の合わせ方
    fit: BackgroundFit,
    /// `FitWidth` で動画サイズが判明した後のズーム計算待ちか
    pending_fit_width: bool,
    /// 埋め込み先ウィンドウのサイズ (物理ピクセル)
    window_size: (i32, i32),
}

impl VideoPlayer {
    /// 指定ウィンドウ (HWND) に埋め込んでプレイヤーを構築する
    ///
    /// `window_size` は埋め込み先の幅と高さ (物理ピクセル)。
    pub fn new(
        window: HWND,
        window_size: (i32, i32),
        muted: bool,
        fit: BackgroundFit,
    ) -> Result<Self> {
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

        let mut player = Self {
            mpv,
            muted,
            fit,
            pending_fit_width: false,
            window_size,
        };
        player.apply_fit(fit)?;
        Ok(player)
    }

    /// 動画を読み込んで再生を開始する
    pub fn load_file(&mut self, path: &Path) -> Result<()> {
        if !path.is_file() {
            return Err(Error::FileNotFound(path.to_path_buf()));
        }
        let arg = path.to_string_lossy();
        self.mpv.command(&["loadfile", &arg])?;
        // 動画が変わればサイズも変わるため、幅合わせの再計算を予約する
        if self.fit == BackgroundFit::FitWidth {
            self.pending_fit_width = true;
        }
        log::info!("動画を再生します: {arg}");
        Ok(())
    }

    /// 背景の合わせ方を変更する
    pub fn set_fit(&mut self, fit: BackgroundFit) -> Result<()> {
        if self.fit == fit && !self.pending_fit_width {
            return Ok(());
        }
        self.apply_fit(fit)
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
        self.try_apply_fit_width();
    }

    /// 背景の合わせ方を mpv プロパティへ反映する
    ///
    /// mpv の既定表示 (`panscan=0`) は動画全体が画面内に収まる「収める」表示。
    /// そこへ各モード固有のプロパティを重ねて目的の見え方にする。
    fn apply_fit(&mut self, fit: BackgroundFit) -> Result<()> {
        // 前モードの影響を消すため共通プロパティを先にリセットする
        self.mpv.set_property("panscan", "0")?;
        self.mpv.set_property("video-zoom", "0")?;
        self.mpv.set_property("video-unscaled", "no")?;
        self.pending_fit_width = false;
        match fit {
            // 画面いっぱいに拡大し、はみ出しはクロップする
            BackgroundFit::Cover => self.mpv.set_property("panscan", "1.0")?,
            // 幅を基準に拡大する。倍率は動画サイズ確定後に計算する
            BackgroundFit::FitWidth => self.pending_fit_width = true,
            // 既定の「収める」表示のまま
            BackgroundFit::FitScreen => {}
            // 等倍で中央に固定する (リサイズもされない)
            BackgroundFit::Center => self.mpv.set_property("video-unscaled", "yes")?,
        }
        self.fit = fit;
        Ok(())
    }

    /// `FitWidth` 用のズームを、動画サイズが判明したら計算して適用する
    fn try_apply_fit_width(&mut self) {
        if !self.pending_fit_width {
            return;
        }
        // 動画サイズはデコード開始後しか取れないので、取れるまで毎フレーム再試行する
        let (Ok(w), Ok(h)) = (
            self.mpv.get_property("video-params/w"),
            self.mpv.get_property("video-params/h"),
        ) else {
            return;
        };
        let (Ok(w), Ok(h)) = (w.parse::<i32>(), h.parse::<i32>()) else {
            self.pending_fit_width = false;
            log::warn!("動画サイズ ({w}x{h}) を解釈できず幅合わせを諦めました");
            return;
        };
        self.pending_fit_width = false;
        let zoom = fit_width_zoom(self.window_size.0, self.window_size.1, w, h);
        if let Err(e) = self.mpv.set_property("video-zoom", &format!("{zoom}")) {
            log::warn!("video-zoom の設定に失敗しました: {e}");
        } else {
            log::debug!("ページ幅合わせのため video-zoom={zoom} を適用 (動画 {w}x{h})");
        }
    }
}

/// 幅を基準に拡大するときの `video-zoom` 値を求める
///
/// mpv の既定表示は動画全体を画面内に収める (contain) ので、その表示幅から
/// 画面幅いっぱいまで広げる追加倍率を 2 のべき乗 (`2^zoom`) で返す。
/// 動画のほうが相対的に横長なら幅は既に合っているので 0 (等倍) を返す。
fn fit_width_zoom(win_w: i32, win_h: i32, vid_w: i32, vid_h: i32) -> f64 {
    if win_w <= 0 || win_h <= 0 || vid_w <= 0 || vid_h <= 0 {
        return 0.0;
    }
    let s = f64::min(win_w as f64 / vid_w as f64, win_h as f64 / vid_h as f64);
    let factor = win_w as f64 / (vid_w as f64 * s);
    factor.log2()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_width_zoom_fills_width_for_tall_video() {
        // 正方形動画を 16:9 画面へ収めると幅 1080/1920 → 拡大して幅を合わせる
        let zoom = fit_width_zoom(1920, 1080, 1000, 1000);
        let factor = 2f64.powf(zoom);
        assert!((factor - 1920.0 / 1080.0).abs() < 1e-9);
    }

    #[test]
    fn fit_width_zoom_is_zero_when_video_already_matches_or_exceeds_width() {
        // 16:9 動画を 16:9 画面に収めると幅がぴったり一致する
        assert_eq!(fit_width_zoom(1920, 1080, 1920, 1080), 0.0);
        // 相対的に横長な動画は contain でも幅いっぱいになる
        assert_eq!(fit_width_zoom(1920, 1080, 4000, 1000), 0.0);
    }

    #[test]
    fn fit_width_zoom_guards_invalid_sizes() {
        for size in [
            (0, 1080, 1000, 1000),
            (1920, -1, 1000, 1000),
            (1920, 1080, 0, 100),
        ] {
            let (w, h, vw, vh) = size;
            assert_eq!(fit_width_zoom(w, h, vw, vh), 0.0);
        }
    }
}
