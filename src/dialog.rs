//! ネイティブのファイル選択ダイアログ共通処理

use std::path::PathBuf;

/// 壁紙動画を選ぶファイル選択ダイアログを開く
pub fn pick_video_file() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("壁紙動画を選択")
        .add_filter(
            "動画ファイル",
            &[
                "mp4", "webm", "mkv", "mov", "avi", "flv", "wmv", "m4v", "ts",
            ],
        )
        .add_filter("すべてのファイル", &["*"])
        .pick_file()
}
