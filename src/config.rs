//! 設定ファイル (`%APPDATA%\livetop\config.toml`) の読み書き

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// アプリの表示名 (レジストリの値名などにも使用)
pub const APP_NAME: &str = "Livetop";

/// 設定ファイル名
pub const CONFIG_FILE: &str = "config.toml";

/// 壁紙をどのディスプレイに適用するか
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayMode {
    /// 全画面 (全モニタ) にまたがって表示する
    Spanning,
    /// 各ディスプレイ毎に個別表示する
    PerDisplay,
    /// 指定したディスプレイのみに表示する (`Config::display_index` 参照)
    Specific,
}

impl DisplayMode {
    /// 表示名 (設定画面用)
    pub fn label(&self) -> &'static str {
        match self {
            Self::Spanning => "全画面にまたがって表示",
            Self::PerDisplay => "各ディスプレイ毎に表示",
            Self::Specific => "指定ディスプレイのみに表示",
        }
    }
}

/// 永続化する設定
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// 壁紙として再生する動画のパス
    pub video_path: Option<PathBuf>,
    /// Windows 起動時に自動起動するか
    pub autostart: bool,
    /// 音声をミュートにするか
    pub muted: bool,
    /// ディスプレイへの適用方法
    pub display_mode: DisplayMode,
    /// `Specific` モードで使用するディスプレイの番号 (0 始まり)
    pub display_index: usize,
    /// `PerDisplay` モードでディスプレイ毎に指定する動画 (キーはディスプレイ番号)
    ///
    /// 指定のないディスプレイには `video_path` が使われる。
    pub display_videos: BTreeMap<usize, PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            video_path: None,
            autostart: false,
            // 壁紙用途では初期状態でミュート (低負荷のため音声デコードも無効化)
            muted: true,
            // 初期設定はメインディスプレイのみに表示する
            display_mode: DisplayMode::Specific,
            display_index: crate::wallpaper::primary_display_index(),
            display_videos: BTreeMap::new(),
        }
    }
}

impl Config {
    /// 指定ディスプレイで再生すべき動画を求める
    ///
    /// `PerDisplay` モードではディスプレイ毎の指定を優先し、
    /// それ以外のモードや指定の無いディスプレイでは既定の `video_path` を使う。
    pub fn video_for_display(&self, display_index: usize) -> Option<PathBuf> {
        if self.display_mode == DisplayMode::PerDisplay
            && let Some(path) = self.display_videos.get(&display_index)
        {
            return Some(path.clone());
        }
        self.video_path.clone()
    }
}

/// 設定ファイルを置くディレクトリ (`%APPDATA%\livetop`)
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_NAME.to_lowercase())
}

/// 設定ファイルのパス
pub fn config_path() -> PathBuf {
    config_dir().join(CONFIG_FILE)
}

impl Config {
    /// 設定を読み込む。ファイルが無い・壊れている場合は既定値を使う。
    pub fn load() -> Self {
        let path = config_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => match toml::from_str(&text) {
                Ok(cfg) => cfg,
                Err(e) => {
                    log::warn!("設定ファイルの解析に失敗したため既定値を使います: {path:?} ({e})");
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        }
    }

    /// 設定を保存する (親ディレクトリが無ければ作成する)。
    pub fn save(&self) -> Result<()> {
        let path = config_path();
        let dir = config_dir();
        std::fs::create_dir_all(&dir)?;
        let text = toml::to_string_pretty(self).map_err(|e| Error::ConfigEncode(e.to_string()))?;
        std::fs::write(&path, text)?;
        log::debug!("設定を保存しました: {path:?}");
        Ok(())
    }

    /// 動画パスが空文字でない実在するパスかどうか
    pub fn has_valid_video(&self) -> bool {
        let valid = |p: &Path| !p.as_os_str().is_empty() && p.exists();
        self.video_path.as_deref().is_some_and(valid)
            || self.display_videos.values().any(|p| valid(p))
    }
}
