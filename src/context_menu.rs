use std::path::Path;

use crate::error::{Error, Result};

// ---------------------------------------------------------------------------
// Windows: Explorer context menu via registry (HKCU, no admin rights).
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod windows_impl {
    use super::*;
    use std::path::Path;
    use std::process::Command;

    const CLIPBOARD_KEY: &str =
        r"HKEY_CURRENT_USER\Software\Classes\Directory\shell\pack2text_clipboard";
    const TEXT_KEY: &str = r"HKEY_CURRENT_USER\Software\Classes\Directory\shell\pack2text_text";
    const STAR_CLIPBOARD_KEY: &str =
        r"HKEY_CURRENT_USER\Software\Classes\*\shell\pack2text_clipboard";
    const STAR_TEXT_KEY: &str = r"HKEY_CURRENT_USER\Software\Classes\*\shell\pack2text_text";

    fn run_reg(args: &[&str]) -> Result<()> {
        let mut cmd = Command::new("reg");
        cmd.args(args);
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
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

    pub fn install(exe_path: &Path) -> Result<()> {
        let exe = exe_path.to_string_lossy().to_string();
        let clipboard_cmd = format!("\"{exe}\" --menu --clipboard --menu-dir \"%V\" \"%1\"");
        let text_cmd = format!("\"{exe}\" --menu --menu-dir \"%V\" \"%1\"");
        add_entry(CLIPBOARD_KEY, "Pack to Clipboard", &clipboard_cmd)?;
        add_entry(TEXT_KEY, "Pack to Text", &text_cmd)?;
        add_entry(STAR_CLIPBOARD_KEY, "Pack to Clipboard", &clipboard_cmd)?;
        add_entry(STAR_TEXT_KEY, "Pack to Text", &text_cmd)?;
        Ok(())
    }

    pub fn uninstall() -> Result<()> {
        for key in [CLIPBOARD_KEY, TEXT_KEY, STAR_CLIPBOARD_KEY, STAR_TEXT_KEY] {
            let _ = run_reg(&["delete", key, "/f"]);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// macOS: Finder Quick Actions via Automator workflows (~/Library/Services).
// Each workflow is a plain plist bundle that runs a shell script, so no
// Xcode/Swift project is needed. The system registers .workflow bundles in
// ~/Library/Services as right-click "Quick Actions" on files and folders.
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
mod macos_impl {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    const SERVICES_DIR: &str = "Library/Services";

    fn services_dir() -> Result<PathBuf> {
        let home =
            std::env::var("HOME").map_err(|_| Error::Platform("HOME is not set".to_string()))?;
        Ok(PathBuf::from(home).join(SERVICES_DIR))
    }

    fn workflow_plist(command: &str) -> String {
        // A minimal Automator "Run Shell Script" Quick Action that receives
        // Finder's selected files/folders as arguments. The shell script is
        // exactly what we install; no trailing content expected.
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>AMApplicationBuild</key>
	<string>506</string>
	<key>AMApplicationVersion</key>
	<string>2.10</string>
	<key>AMDocumentVersion</key>
	<string>2</string>
	<key>actions</key>
	<array>
		<dict>
			<key>action</key>
			<dict>
				<key>AMAccepts</key>
				<dict>
					<key>Container</key>
					<string>List</string>
					<key>Optional</key>
					<false/>
					<key>Types</key>
					<array>
						<string>com.apple.cocoa.string</string>
					</array>
				</dict>
				<key>AMActionVersion</key>
				<string>2.0.3</string>
				<key>AMApplication</key>
				<array>
					<string>Automator</string>
				</array>
				<key>AMParameterProperties</key>
				<dict>
					<key>COMMAND_STRING</key>
					<dict/>
					<key>CheckedForUserDefaultShell</key>
					<dict/>
					<key>inputMethod</key>
					<dict/>
					<key>shell</key>
					<dict/>
					<key>source</key>
					<dict/>
				</dict>
				<key>AMProvides</key>
				<dict>
					<key>Container</key>
					<string>List</string>
					<key>Types</key>
					<array>
						<string>com.apple.cocoa.string</string>
					</array>
				</dict>
				<key>ActionBundlePath</key>
				<string>/System/Library/Automator/Run Shell Script.action</string>
				<key>ActionName</key>
				<string>Run Shell Script</string>
				<key>ActionParameters</key>
				<dict>
					<key>COMMAND_STRING</key>
					<string>{command}</string>
					<key>CheckedForUserDefaultShell</key>
					<true/>
					<key>inputMethod</key>
					<integer>1</integer>
					<key>shell</key>
					<string>/bin/bash</string>
					<key>source</key>
					<string>{command}</string>
				</dict>
				<key>BundleIdentifier</key>
				<string>com.apple.RunShellScript</string>
				<key>CFBundleVersion</key>
				<string>2.0.3</string>
				<key>CanShowSelectedItemsWhenRun</key>
				<false/>
				<key>CanShowWhenRun</key>
				<true/>
				<key>Category</key>
				<array>
					<string>AMCategoryUtilities</string>
				</array>
				<key>Class Name</key>
				<string>RunShellScriptAction</string>
				<key>InputUUID</key>
				<string>00000000-0000-0000-0000-000000000001</string>
				<key>Keywords</key>
				<array>
					<string>Shell</string>
					<string>Script</string>
					<string>Command</string>
				</array>
				<key>OutputUUID</key>
				<string>00000000-0000-0000-0000-000000000002</string>
				<key>UUID</key>
				<string>00000000-0000-0000-0000-000000000003</string>
			</dict>
			<key>isViewVisible</key>
			<true/>
		</dict>
	</array>
	<key>connectors</key>
	<dict/>
	<key>workflowMetaData</key>
	<dict>
		<key>serviceInputTypeIdentifier</key>
		<string>com.apple.Automator.fileSystemObject</string>
		<key>serviceOutputTypeIdentifier</key>
		<string>com.apple.Automator.nothing</string>
		<key>serviceProcessesInput</key>
		<integer>0</integer>
		<key>workflowTypeIdentifier</key>
		<string>com.apple.Automator.servicesMenu</string>
	</dict>
</dict>
</plist>
"#
        )
    }

    fn info_plist(bundle_id: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleIdentifier</key>
	<string>dev.bobwen.pack2text.{bundle_id}</string>
	<key>CFBundleName</key>
	<string>pack2text</string>
	<key>CFBundleVersion</key>
	<string>1.0</string>
	<key>CFBundleShortVersionString</key>
	<string>1.0</string>
</dict>
</plist>
"#
        )
    }

    fn write_workflow(name: &str, bundle_id: &str, command: &str) -> Result<PathBuf> {
        let dir = services_dir()?;
        fs::create_dir_all(&dir)?;
        let wf = dir.join(format!("{name}.workflow"));
        if wf.exists() {
            fs::remove_dir_all(&wf)?;
        }
        let contents = wf.join("Contents");
        fs::create_dir_all(&contents)?;
        fs::write(contents.join("document.wflow"), workflow_plist(command))?;
        fs::write(contents.join("Info.plist"), info_plist(bundle_id))?;
        Ok(wf)
    }

    pub fn install(exe_path: &Path) -> Result<()> {
        let exe = exe_path.to_string_lossy();
        let clipboard_cmd = format!("\"{exe}\" --menu --clipboard \"$@\"");
        let text_cmd = format!("\"{exe}\" --menu \"$@\"");
        write_workflow("Pack to Clipboard", "clipboard", &clipboard_cmd)?;
        write_workflow("Pack to Text", "text", &text_cmd)?;
        Ok(())
    }

    pub fn uninstall() -> Result<()> {
        let dir = services_dir()?;
        for name in ["Pack to Clipboard", "Pack to Text"] {
            let wf = dir.join(format!("{name}.workflow"));
            if wf.exists() {
                fs::remove_dir_all(&wf)?;
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------

pub fn install_menu(exe_path: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        macos_impl::install(exe_path)
    }
    #[cfg(windows)]
    {
        windows_impl::install(exe_path)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = exe_path;
        Err(Error::Platform(
            "context menu is not supported on this platform".to_string(),
        ))
    }
}

pub fn uninstall_menu() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        macos_impl::uninstall()
    }
    #[cfg(windows)]
    {
        windows_impl::uninstall()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Err(Error::Platform(
            "context menu is not supported on this platform".to_string(),
        ))
    }
}
