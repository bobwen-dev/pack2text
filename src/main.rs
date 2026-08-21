#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cli;
mod collect;
mod context_menu;
mod encoding;
mod error;
mod format;
mod pack;
mod unpack;

use std::fs;
use std::path::{Path, PathBuf};

use clap::Parser;
use cli::Cli;
use error::Error;

/// On Windows the release build has no console (`windows_subsystem`), so a
/// plain eprintln is invisible when invoked from the Explorer context menu.
/// Surface the error in a message box unless a console is attached.
#[cfg(windows)]
fn notify_error(msg: &str) {
    use windows_sys::Win32::System::Console::GetConsoleWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    unsafe {
        if !GetConsoleWindow().is_null() {
            return;
        }
        let wide: Vec<u16> = msg.encode_utf16().chain(std::iter::once(0)).collect();
        let title: Vec<u16> = "pack2text"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        MessageBoxW(
            std::ptr::null_mut(),
            wide.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(not(windows))]
fn notify_error(_msg: &str) {}

fn fail(msg: impl std::fmt::Display) -> ! {
    let msg = format!("error: {msg}");
    eprintln!("{msg}");
    notify_error(&msg);
    std::process::exit(1);
}

fn main() {
    let cli = Cli::parse();

    if let Some(container_path) = &cli.unpack {
        let out_dir = cli.unpack_dir.as_deref().unwrap_or(Path::new("unpacked"));
        let container = match fs::read_to_string(container_path) {
            Ok(c) => c,
            Err(e) => {
                fail(format!(
                    "cannot read {}: {e}",
                    Path::new(container_path).display()
                ));
            }
        };
        match unpack::unpack_to_dir(&container, out_dir) {
            Ok(count) => {
                println!("extracted {count} files to {}", out_dir.display());
            }
            Err(e) => fail(e),
        }
        return;
    }

    if cli.install_menu {
        let exe = match std::env::current_exe() {
            Ok(p) => p,
            Err(e) => fail(format!("cannot determine exe path: {e}")),
        };
        if let Err(e) = context_menu::install_menu(&exe) {
            fail(e);
        }
        println!("context menu installed");
        return;
    }

    if cli.uninstall_menu {
        if let Err(e) = context_menu::uninstall_menu() {
            fail(e);
        }
        println!("context menu uninstalled");
        return;
    }

    if cli.directories.is_empty() {
        fail("no directories specified\nusage: pack2text <DIRECTORIES...> [OPTIONS]");
    }

    if cli.clipboard && cli.output.is_some() {
        fail("--clipboard and --output are mutually exclusive");
    }

    let dirs: Vec<&Path> = cli.directories.iter().map(PathBuf::as_path).collect();

    if cli.menu {
        run_menu(&cli, &dirs);
        return;
    }

    for dir in &dirs {
        if !dir.is_dir() {
            fail(format!("not a directory: {}", dir.display()));
        }
    }

    let output_path = if cli.clipboard {
        None
    } else {
        let path = match &cli.output {
            Some(p) => p.clone(),
            None => {
                let default_name = pack::default_output_name(&cli.directories);
                PathBuf::from(default_name)
            }
        };
        if path.exists() && !cli.force {
            fail(format!(
                "output file already exists: {} (use -f to overwrite)",
                path.display()
            ));
        }
        Some(path)
    };

    let result = pack::pack_directories(
        &dirs,
        cli.include.as_deref(),
        cli.exclude.as_deref(),
        output_path.as_deref(),
        cli.clipboard,
    );

    match result {
        Ok(res) => {
            if cli.verbose {
                eprintln!("packed {} files", res.file_count);
            }

            if cli.clipboard {
                push_to_clipboard(res.container, cli.verbose);
            } else if let Some(path) = &output_path {
                if let Err(e) = fs::write(path, &res.container) {
                    fail(format!("cannot write {}: {e}", path.display()));
                }

                if cli.verbose {
                    eprintln!("wrote {}", path.display());
                }
            }
        }
        Err(e) => exit_on_pack_error(e),
    }
}

fn push_to_clipboard(container: String, verbose: bool) {
    match arboard::Clipboard::new() {
        Ok(mut clipboard) => {
            if let Err(e) = clipboard.set_text(container) {
                fail(format!("clipboard: {e}"));
            }
            if verbose {
                eprintln!("copied to clipboard");
            }
        }
        Err(e) => fail(format!("cannot access clipboard: {e}")),
    }
}

fn exit_on_pack_error(e: Error) -> ! {
    match e {
        Error::NoTextFiles => fail("no text files found"),
        e => fail(e),
    }
}

fn run_menu(cli: &Cli, selected: &[&Path]) {
    for sel in selected {
        if !sel.exists() {
            fail(format!("not found: {}", sel.display()));
        }
    }

    let output_path = if cli.clipboard {
        None
    } else {
        let path = match &cli.output {
            Some(p) => p.clone(),
            None => {
                let (output_dir, base_name) =
                    pack::menu_output_location(selected, cli.menu_dir.as_deref());
                output_dir.join(base_name)
            }
        };
        Some(pack::resolve_auto_rename(&path))
    };

    let result = pack::pack_selected(
        selected,
        cli.include.as_deref(),
        cli.exclude.as_deref(),
        output_path.as_deref(),
        cli.clipboard,
    );
    match result {
        Ok(res) => {
            if cli.verbose {
                eprintln!("packed {} files", res.file_count);
            }
            if cli.clipboard {
                push_to_clipboard(res.container, cli.verbose);
            } else if let Some(path) = &output_path {
                if let Err(e) = fs::write(path, &res.container) {
                    fail(format!("cannot write {}: {e}", path.display()));
                }
                if cli.verbose {
                    eprintln!("wrote {}", path.display());
                }
            }
        }
        Err(e) => exit_on_pack_error(e),
    }
}
