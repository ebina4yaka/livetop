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
        Self::load_from(&config_path())
    }

    /// 指定パスから設定を読み込む (テスト用に分離)。
    fn load_from(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
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
        self.save_to(&config_path())
    }

    /// 指定パスへ設定を保存する (テスト用に分離)。
    fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| Error::ConfigEncode(e.to_string()))?;
        std::fs::write(path, text)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn config(mode: DisplayMode) -> Config {
        Config {
            display_mode: mode,
            ..Config::default()
        }
    }

    /// テスト用の一時ファイルパス (衝突しないよう時刻とプロセス ID を付ける)
    fn temp_path(name: &str) -> PathBuf {
        let unique = format!(
            "livetop-test-{name}-{}-{:?}.toml",
            std::process::id(),
            std::time::SystemTime::now()
        );
        std::env::temp_dir().join(unique)
    }

    #[test]
    fn video_for_display_falls_back_to_video_path_outside_per_display() {
        let mut c = config(DisplayMode::Specific);
        c.video_path = Some("main.mp4".into());
        c.display_videos.insert(0, "per0.mp4".into());
        // Specific モードでは display_videos は無視される
        assert_eq!(c.video_for_display(0), Some(PathBuf::from("main.mp4")));
        assert_eq!(c.video_for_display(1), Some(PathBuf::from("main.mp4")));
    }

    #[test]
    fn video_for_display_prefers_display_video_in_per_display_mode() {
        let mut c = config(DisplayMode::PerDisplay);
        c.video_path = Some("main.mp4".into());
        c.display_videos.insert(0, "per0.mp4".into());
        // 個別指定のあるディスプレイはそれを優先する
        assert_eq!(c.video_for_display(0), Some(PathBuf::from("per0.mp4")));
        // 指定の無いディスプレイは video_path にフォールバックする
        assert_eq!(c.video_for_display(1), Some(PathBuf::from("main.mp4")));
        assert_eq!(c.video_for_display(2), Some(PathBuf::from("main.mp4")));
    }

    #[test]
    fn video_for_display_returns_none_without_any_video() {
        let c = config(DisplayMode::PerDisplay);
        assert_eq!(c.video_for_display(0), None);
    }

    #[test]
    fn display_mode_serde_names() {
        // TOML はトップレベルに enum を置けないため Config 経由で検証する
        let names = [
            (DisplayMode::Spanning, "spanning"),
            (DisplayMode::PerDisplay, "per_display"),
            (DisplayMode::Specific, "specific"),
        ];
        for (mode, name) in names {
            let c = config(mode);
            let text = toml::to_string_pretty(&c).unwrap();
            let parsed: Config = toml::from_str(&text).unwrap();
            assert_eq!(parsed.display_mode, mode);
            assert!(text.contains(&format!("display_mode = \"{name}\"")));
        }
    }

    #[test]
    fn config_defaults() {
        let c = Config::default();
        assert!(c.video_path.is_none());
        assert!(!c.autostart);
        assert!(c.muted, "壁紙用途の初期設定はミュート");
        assert_eq!(c.display_mode, DisplayMode::Specific);
        assert!(c.display_videos.is_empty());
    }

    #[test]
    fn config_roundtrip() {
        let c = Config {
            video_path: Some(PathBuf::from(r"C:\videos\test.mp4")),
            autostart: true,
            muted: false,
            display_mode: DisplayMode::PerDisplay,
            display_index: 1,
            display_videos: BTreeMap::from([
                (0, PathBuf::from(r"C:\videos\per0.mp4")),
                (2, PathBuf::from(r"C:\videos\per2.mp4")),
            ]),
        };
        let text = toml::to_string_pretty(&c).unwrap();
        let parsed: Config = toml::from_str(&text).unwrap();
        assert_eq!(parsed, c);
    }

    #[test]
    fn save_then_load_roundtrip() {
        let path = temp_path("roundtrip");
        let c = Config {
            video_path: Some(PathBuf::from("some-video.mp4")),
            autostart: true,
            muted: false,
            display_mode: DisplayMode::Spanning,
            display_index: 0,
            display_videos: BTreeMap::from([(1, PathBuf::from("per1.mp4"))]),
        };
        c.save_to(&path).unwrap();
        assert_eq!(Config::load_from(&path), c);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_from_uses_default_when_file_missing() {
        assert_eq!(Config::load_from(&temp_path("missing")), Config::default());
    }

    #[test]
    fn load_from_uses_default_when_toml_is_broken() {
        let path = temp_path("broken");
        std::fs::write(&path, "this is {{{ not toml").unwrap();
        assert_eq!(Config::load_from(&path), Config::default());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_from_accepts_partial_config() {
        let path = temp_path("partial");
        // 未指定フィールドは既定値で埋まる
        std::fs::write(&path, "muted = false\n").unwrap();
        let c = Config::load_from(&path);
        assert!(!c.muted);
        assert_eq!(c.video_path, None);
        assert_eq!(c.display_mode, DisplayMode::Specific);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn has_valid_video_checks_existence() {
        let existing = temp_path("existing-video");
        std::fs::write(&existing, "dummy").unwrap();

        let mut c = Config::default();
        assert!(!c.has_valid_video());

        c.video_path = Some(existing.clone());
        assert!(c.has_valid_video());

        c.video_path = Some(PathBuf::from(""));
        assert!(!c.has_valid_video(), "空パスは無効");

        c.video_path = None;
        c.display_videos.insert(0, existing.clone());
        assert!(c.has_valid_video());

        c.display_videos.clear();
        c.display_videos.insert(0, temp_path("does-not-exist"));
        assert!(!c.has_valid_video());

        let _ = std::fs::remove_file(&existing);
    }
}
