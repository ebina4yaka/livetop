//! アプリ全体で共通して使うエラー型

use std::path::PathBuf;

/// アプリ全体のエラー型
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("mpv ライブラリ (libmpv-2.dll) をロードできませんでした: {0}")]
    MpvLibrary(String),

    #[error("mpv の DLL に必須シンボルがありません: {0}")]
    MpvMissingSymbol(String),

    #[error("mpv の呼び出しに失敗しました: {0} (code={1})")]
    Mpv(String, i32),

    #[error("設定ファイルの読み書きに失敗しました: {0}")]
    Config(#[from] std::io::Error),

    #[error("設定ファイルの保存に失敗しました: {0}")]
    ConfigEncode(String),

    #[error("レジストリ操作に失敗しました: {0}")]
    Registry(String),

    #[error("動画ファイルが存在しません: {0}")]
    FileNotFound(PathBuf),

    #[error("トレイアイコンの作成に失敗しました: {0}")]
    Tray(String),
}

/// アプリ全体の `Result` エイリアス
pub type Result<T> = std::result::Result<T, Error>;
