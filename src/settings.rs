//! 設定ウィンドウ (egui / eframe)
//!
//! メインスレッドで動く必要がある winit/eframe の制約のため、`livetop --settings`
//! として別プロセスで起動し、設定は config.toml に書き込む。
//! メインプロセスは設定ファイルの変更を監視して反映する。

use crate::config::{BackgroundFit, Config, DisplayMode};
use ab_glyph::Font;
use egui::FontFamily;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// 設定ウィンドウを表示する (`--settings` モードのエントリポイント)
pub fn run_settings_window() -> Result<(), eframe::Error> {
    let config = Config::load();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Livetop 設定")
            .with_inner_size([520.0, 580.0])
            .with_min_inner_size([400.0, 420.0]),
        ..Default::default()
    };

    eframe::run_native(
        "livetop-settings",
        options,
        Box::new(move |cc| {
            // egui の既定フォントには日本語が含まれないため、システムの日本語フォントを読み込む
            setup_japanese_font(&cc.egui_ctx);
            Ok(Box::new(SettingsApp::new(config)))
        }),
    )
}

/// システムから日本語フォントを探して egui に読み込む
///
/// egui の既定フォントには日本語グリフが無く、そのままだと日本語が「□」で
/// 表示される。Windows に同梱されている日本語フォントをフォールバックとして
/// 追加する (ラテン文字は既定フォントのまま、不足する日本語だけを補う)。
fn setup_japanese_font(ctx: &egui::Context) {
    let Some(bytes) = find_japanese_font() else {
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "japanese".to_owned(),
        egui::FontData::from_owned(bytes).into(),
    );
    // 既定フォント(ラテン等)の後に追加し、足りない文字だけ日本語で補う
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("japanese".to_owned());
    }
    ctx.set_fonts(fonts);
}

/// パース可能かつ日本語グリフを持つフォントファイルを探す
fn find_japanese_font() -> Option<Vec<u8>> {
    const CANDIDATES: &[&str] = &[
        r"C:\Windows\Fonts\NotoSansJP-VF.ttf",
        r"C:\Windows\Fonts\YuGothR.ttc",
        r"C:\Windows\Fonts\YuGothM.ttc",
        r"C:\Windows\Fonts\msgothic.ttc",
        r"C:\Windows\Fonts\meiryo.ttc",
    ];

    for path in CANDIDATES {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        // ab_glyph で実際にパースできるか検証する (壊れた/非対応フォントで egui が
        // パニックしないように)。日本語グリフ「設」が無いフォントもスキップする。
        let Ok(font) = ab_glyph::FontRef::try_from_slice(&bytes) else {
            continue;
        };
        if font.glyph_id('設').0 == 0 {
            continue;
        }
        log::info!("日本語フォントを読み込みました: {path}");
        return Some(bytes);
    }
    log::warn!("日本語フォントが見つかりませんでした (日本語が四角で表示されます)");
    None
}

/// 設定ウィンドウの UI
struct SettingsApp {
    video_path: String,
    /// ディスプレイ毎の動画 (キーはディスプレイ番号、空文字は未指定)
    display_videos: BTreeMap<usize, String>,
    autostart: bool,
    muted: bool,
    background_fit: BackgroundFit,
    display_mode: DisplayMode,
    display_index: usize,
    message: String,
}

impl SettingsApp {
    fn new(config: Config) -> Self {
        Self {
            video_path: config
                .video_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            display_videos: config
                .display_videos
                .iter()
                .map(|(k, v)| (*k, v.display().to_string()))
                .collect(),
            autostart: config.autostart,
            muted: config.muted,
            background_fit: config.background_fit,
            display_mode: config.display_mode,
            display_index: config.display_index,
            message: String::new(),
        }
    }

    /// フォームの値を設定ファイルへ保存する
    fn apply(&mut self) -> bool {
        let video_path = self.video_path.trim();
        let display_videos = self
            .display_videos
            .iter()
            .filter_map(|(k, v)| {
                let v = v.trim();
                (!v.is_empty()).then(|| (*k, PathBuf::from(v)))
            })
            .collect();
        let new_cfg = Config {
            video_path: if video_path.is_empty() {
                None
            } else {
                Some(video_path.into())
            },
            display_videos,
            autostart: self.autostart,
            muted: self.muted,
            background_fit: self.background_fit,
            display_mode: self.display_mode,
            display_index: self.display_index,
        };
        match new_cfg.save() {
            Ok(()) => {
                self.message = "保存しました。".to_string();
                true
            }
            Err(e) => {
                self.message = format!("保存に失敗しました: {e}");
                false
            }
        }
    }
}

impl eframe::App for SettingsApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(4.0);
            ui.heading("Livetop 設定");
            ui.add_space(8.0);

            egui::Grid::new("settings")
                .num_columns(2)
                .spacing([8.0, 12.0])
                .show(ui, |ui| {
                    ui.label("壁紙動画");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.video_path)
                                .hint_text("動画ファイルのパス"),
                        );
                        if ui.button("参照...").clicked()
                            && let Some(path) = crate::dialog::pick_video_file()
                        {
                            self.video_path = path.display().to_string();
                        }
                    });
                    ui.end_row();

                    ui.label("自動起動");
                    ui.checkbox(&mut self.autostart, "Windows 起動時に自動で適用する");
                    ui.end_row();

                    ui.label("音声");
                    ui.checkbox(&mut self.muted, "ミュート (低負荷)");
                    ui.end_row();

                    ui.label("背景の表示方法");
                    ui.vertical(|ui| {
                        for fit in [
                            BackgroundFit::FitWidth,
                            BackgroundFit::FitScreen,
                            BackgroundFit::Cover,
                            BackgroundFit::Center,
                        ] {
                            ui.radio_value(&mut self.background_fit, fit, fit.label());
                        }
                    });
                    ui.end_row();

                    ui.label("表示モード");
                    ui.vertical(|ui| {
                        ui.radio_value(
                            &mut self.display_mode,
                            DisplayMode::Spanning,
                            "全画面にまたがって表示",
                        );
                        ui.radio_value(
                            &mut self.display_mode,
                            DisplayMode::PerDisplay,
                            "各ディスプレイ毎に表示",
                        );
                        ui.horizontal(|ui| {
                            ui.radio_value(
                                &mut self.display_mode,
                                DisplayMode::Specific,
                                "指定ディスプレイのみ",
                            );
                            if self.display_mode == DisplayMode::Specific {
                                self.display_selector(ui);
                            }
                        });
                    });
                    ui.end_row();
                });

            // 「各ディスプレイ毎に表示」のときだけ、ディスプレイ毎の動画を編集できる
            if self.display_mode == DisplayMode::PerDisplay {
                self.display_videos_editor(ui);
            }

            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("適用").clicked() && self.apply() {
                    // 保存できたらウィンドウを閉じる
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if !self.message.is_empty() {
                    ui.label(&self.message);
                }
            });
            ui.add_space(4.0);
            ui.small("適用すると即座に壁紙動画が切り替わります。");
        });
    }
}

impl SettingsApp {
    /// 「指定ディスプレイのみ」で使うディスプレイ選択コンボ
    fn display_selector(&mut self, ui: &mut egui::Ui) {
        let monitors = crate::wallpaper::monitors();
        let selected = format!(
            "ディスプレイ {}",
            self.display_index.min(monitors.len().saturating_sub(1)) + 1
        );
        egui::ComboBox::from_id_salt("display_select")
            .selected_text(selected)
            .show_ui(ui, |ui| {
                for (i, m) in monitors.iter().enumerate() {
                    ui.selectable_value(
                        &mut self.display_index,
                        i,
                        format!("ディスプレイ {} ({}x{})", i + 1, m.width, m.height),
                    );
                }
            });
    }

    /// 「各ディスプレイ毎に表示」で、ディスプレイ毎の動画を編集する
    fn display_videos_editor(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        ui.separator();
        ui.label("ディスプレイ毎の動画 (未指定のディスプレイは「壁紙動画」を使います)");
        let monitors = crate::wallpaper::monitors();
        for (i, m) in monitors.iter().enumerate() {
            let current = self.display_videos.get(&i).cloned().unwrap_or_default();
            ui.horizontal(|ui| {
                ui.label(format!("ディスプレイ {} ({}x{})", i + 1, m.width, m.height));
                let name = std::path::Path::new(&current)
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_default();
                ui.label(if current.is_empty() {
                    "未指定".to_string()
                } else {
                    name
                });
                if ui.button("参照...").clicked()
                    && let Some(path) = crate::dialog::pick_video_file()
                {
                    self.display_videos.insert(i, path.display().to_string());
                }
                if !current.is_empty() && ui.button("クリア").clicked() {
                    self.display_videos.remove(&i);
                }
            });
        }
    }
}
