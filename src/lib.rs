//! Livetop: 動画をデスクトップ壁紙として再生する軽量アプリ
//!
//! - 再生: libmpv (libmpv-2.dll を動的ロード)
//! - 埋め込み: Win32 の WorkerW 技法
//! - 設定: egui (eframe) の設定ウィンドウ + トレイメニュー

pub mod app;
pub mod autostart;
pub mod config;
pub mod dialog;
pub mod error;
pub mod mpv;
pub mod settings;
pub mod tray;
pub mod video;
pub mod wallpaper;
