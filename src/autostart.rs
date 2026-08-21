//! Windows 起動時の自動起動をレジストリの Run キーで制御する

use crate::config::APP_NAME;
use crate::error::{Error, Result};
use winreg::RegKey;
use winreg::enums::HKEY_CURRENT_USER;

/// 自動起動を登録するレジストリキー
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Run キーに登録する値名
const VALUE_NAME: &str = APP_NAME;

/// 自動起動の登録/解除を行う
pub fn set_autostart(enabled: bool) -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(RUN_KEY)
        .map_err(|e| Error::Registry(e.to_string()))?;

    if enabled {
        let exe = std::env::current_exe().map_err(|e| Error::Registry(e.to_string()))?;
        let command = format!("\"{}\"", exe.display());
        key.set_value(VALUE_NAME, &command)
            .map_err(|e| Error::Registry(e.to_string()))?;
        log::info!("自動起動を有効にしました: {command}");
    } else {
        if let Ok(()) = key.delete_value(VALUE_NAME) {
            log::info!("自動起動を無効にしました");
        }
    }
    Ok(())
}
