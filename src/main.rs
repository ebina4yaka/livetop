//! Livetop のエントリポイント
//!
//! 使い方:
//!   livetop                 設定ファイルの動画を壁紙として再生
//!   livetop <動画パス>       指定した動画を壁紙として再生 (設定にも保存)
//!   livetop --settings       設定ウィンドウだけを開いて終了

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use livetop::app;
use log::{LevelFilter, Log, Metadata, Record};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::Mutex;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_logging();
    set_panic_hook();

    let mut args = std::env::args();
    let _program = args.next();
    let first = args.next();

    // 設定ウィンドウ専用プロセス
    if first.as_deref() == Some("--settings") {
        return livetop::settings::run_settings_window().map_err(|e| e.to_string().into());
    }

    // 通常起動 (任意で動画パスを指定)
    let video_arg = first.filter(|a| !a.starts_with('-'));
    app::run(video_arg);
    Ok(())
}

/// ログをファイル (`%APPDATA%\livetop\livetop.log`) とコンソールの両方へ出力する
fn setup_logging() {
    let log_dir = livetop::config::config_dir();
    let _ = std::fs::create_dir_all(&log_dir);
    let path = log_dir.join("livetop.log");
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok();
    let logger = FileLogger {
        file: Mutex::new(file),
    };
    // env_logger の代わりに自作ロガーを使う (GUI 常駐でもログが追えるように)
    let _ = log::set_boxed_logger(Box::new(logger));
    let level = if std::env::var_os("RUST_LOG").is_some() {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    };
    log::set_max_level(level);
}

/// パニックを `panic.log` に記録する
fn set_panic_hook() {
    let panic_log = livetop::config::config_dir().join("panic.log");
    std::panic::set_hook(Box::new(move |info| {
        let message = format!("panic: {info}\n");
        eprintln!("{message}");
        if let Some(parent) = panic_log.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&panic_log)
            .and_then(|mut f| f.write_all(message.as_bytes()));
    }));
}

/// ファイル + コンソールへ書き出す最小限のロガー
struct FileLogger {
    file: Mutex<Option<File>>,
}

impl Log for FileLogger {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &Record<'_>) {
        let line = format!(
            "{} [{:<5}] {}: {}\n",
            timestamp(),
            record.level().as_str(),
            record.target(),
            record.args()
        );
        eprint!("{line}");
        if let Some(file) = self.file.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
            let _ = file.write_all(line.as_bytes());
        }
    }

    fn flush(&self) {
        if let Some(file) = self.file.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
            let _ = file.flush();
        }
    }
}

/// `HH:MM:SS` 形式の現在時刻
fn timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let h = (secs / 3600) % 24;
    let m = (secs / 60) % 60;
    let s = secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}
