//! タスクトレイのアイコンとメニュー

use crate::error::{Error, Result};
use muda::{Menu, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder};

/// 自動起動メニューに表示する文字列
fn autostart_label(enabled: bool) -> String {
    if enabled {
        "自動起動: ON".to_string()
    } else {
        "自動起動: OFF".to_string()
    }
}

/// トレイメニューの各項目 ID
pub struct TrayMenu {
    pub settings_id: MenuId,
    pub change_video_id: MenuId,
    pub autostart_id: MenuId,
    pub quit_id: MenuId,
    autostart_item: MenuItem,
}

impl TrayMenu {
    /// 自動起動のメニュー表示を更新する
    pub fn set_autostart_label(&self, enabled: bool) {
        self.autostart_item.set_text(autostart_label(enabled));
    }
}

/// トレイアイコン
pub struct Tray {
    _icon: tray_icon::TrayIcon,
    pub menu: TrayMenu,
}

impl Tray {
    /// トレイアイコンとメニューを構築する
    pub fn build(autostart: bool) -> Result<Self> {
        let menu = Menu::new();

        let settings = MenuItem::new("設定...", true, None);
        let change_video = MenuItem::new("壁紙動画を変更...", true, None);
        let separator1 = PredefinedMenuItem::separator();
        let autostart_item = MenuItem::new(autostart_label(autostart), true, None);
        let separator2 = PredefinedMenuItem::separator();
        let quit = MenuItem::new("終了", true, None);

        menu.append_items(&[
            &settings,
            &change_video,
            &separator1,
            &autostart_item,
            &separator2,
            &quit,
        ])
        .map_err(|e| Error::Tray(e.to_string()))?;

        let icon = build_icon()?;
        let tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Livetop - 動画壁紙")
            .with_icon(icon)
            .build()
            .map_err(|e| Error::Tray(e.to_string()))?;

        let tray_menu = TrayMenu {
            settings_id: settings.id().clone(),
            change_video_id: change_video.id().clone(),
            autostart_id: autostart_item.id().clone(),
            quit_id: quit.id().clone(),
            autostart_item,
        };

        Ok(Self {
            _icon: tray_icon,
            menu: tray_menu,
        })
    }
}

/// トレイアイコン用の画像をコード内で生成する (円形背景 + 再生ボタン)
fn build_icon() -> Result<Icon> {
    const SIZE: usize = 32;
    let mut rgba = vec![0u8; SIZE * SIZE * 4];

    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 + 0.5 - 16.0;
            let dy = y as f32 + 0.5 - 16.0;
            let idx = (y * SIZE + x) * 4;
            // 円形の背景
            if dx * dx + dy * dy <= 15.0 * 15.0 {
                rgba[idx] = 36;
                rgba[idx + 1] = 39;
                rgba[idx + 2] = 48;
                rgba[idx + 3] = 255;
            }
            // 再生ボタンの三角形
            if in_triangle(x as f32 + 0.5, y as f32 + 0.5) {
                rgba[idx] = 0;
                rgba[idx + 1] = 200;
                rgba[idx + 2] = 170;
                rgba[idx + 3] = 255;
            }
        }
    }

    Icon::from_rgba(rgba, SIZE as u32, SIZE as u32).map_err(|e| Error::Tray(e.to_string()))
}

/// 点が再生ボタンの三角形の内側かどうか
fn in_triangle(px: f32, py: f32) -> bool {
    let (ax, ay) = (13.0, 10.0);
    let (bx, by) = (13.0, 22.0);
    let (cx, cy) = (24.0, 16.0);

    let d1 = cross(px, py, ax, ay, bx, by);
    let d2 = cross(px, py, bx, by, cx, cy);
    let d3 = cross(px, py, cx, cy, ax, ay);
    let has_neg = (d1 < 0.0) || (d2 < 0.0) || (d3 < 0.0);
    let has_pos = (d1 > 0.0) || (d2 > 0.0) || (d3 > 0.0);
    !(has_neg && has_pos)
}

/// 三角形 (p, a, b) の外積 (z 成分)
fn cross(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    (bx - ax) * (py - ay) - (by - ay) * (px - ax)
}
