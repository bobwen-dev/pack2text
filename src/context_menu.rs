use std::path::Path;
use std::process::Command;

use crate::error::{Error, Result};

const CLIPBOARD_KEY: &str =
    r"HKEY_CURRENT_USER\Software\Classes\Directory\shell\pack2text_clipboard";
const TEXT_KEY: &str = r"HKEY_CURRENT_USER\Software\Classes\Directory\shell\pack2text_text";
const STAR_CLIPBOARD_KEY: &str = r"HKEY_CURRENT_USER\Software\Classes\*\shell\pack2text_clipboard";
const STAR_TEXT_KEY: &str = r"HKEY_CURRENT_USER\Software\Classes\*\shell\pack2text_text";

fn run_reg(args: &[&str]) -> Result<()> {
    let mut cmd = Command::new("reg");
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let output = cmd
        .output()
        .map_err(|e| Error::Registry(format!("reg command failed: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Registry(format!(
            "reg exited with {}{}{}",
            output.status,
            if stderr.is_empty() { "" } else { ": " },
            stderr.trim()
        )));
    }
    Ok(())
}

fn add_entry(key: &str, label: &str, command: &str) -> Result<()> {
    let result = (|| -> Result<()> {
        run_reg(&["add", key, "/ve", "/d", label, "/f"])?;
        run_reg(&[
            "add",
            key,
            "/v",
            "MultiSelectModel",
            "/t",
            "REG_SZ",
            "/d",
            "Player",
            "/f",
        ])?;
        run_reg(&[
            "add",
            &format!("{key}\\command"),
            "/ve",
            "/d",
            command,
            "/f",
        ])?;
        Ok(())
    })();
    if result.is_err() {
        let _ = run_reg(&["delete", key, "/f"]);
    }
    result
}

pub fn install_menu(exe_path: &Path) -> Result<()> {
    let exe = exe_path.to_string_lossy().to_string();
    let clipboard_cmd = format!("\"{exe}\" --menu --clipboard --menu-dir \"%V\" \"%1\"");
    let text_cmd = format!("\"{exe}\" --menu --menu-dir \"%V\" \"%1\"");
    add_entry(CLIPBOARD_KEY, "Pack to Clipboard", &clipboard_cmd)?;
    add_entry(TEXT_KEY, "Pack to Text", &text_cmd)?;
    add_entry(STAR_CLIPBOARD_KEY, "Pack to Clipboard", &clipboard_cmd)?;
    add_entry(STAR_TEXT_KEY, "Pack to Text", &text_cmd)?;
    Ok(())
}

pub fn uninstall_menu() -> Result<()> {
    for key in [CLIPBOARD_KEY, TEXT_KEY, STAR_CLIPBOARD_KEY, STAR_TEXT_KEY] {
        let _ = run_reg(&["delete", key, "/f"]);
    }
    Ok(())
}
