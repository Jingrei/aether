//! Small Windows integrations: opening the UI window, start-with-Windows, console attachment.

use std::os::windows::process::CommandExt;
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "AetherSound";

/// Opens the control panel as a chromeless Edge app window (falls back to the default browser).
pub fn open_ui(url: &str) {
    let candidates = [
        std::env::var("ProgramFiles(x86)").unwrap_or_default() + r"\Microsoft\Edge\Application\msedge.exe",
        std::env::var("ProgramFiles").unwrap_or_default() + r"\Microsoft\Edge\Application\msedge.exe",
    ];
    for exe in candidates {
        if std::path::Path::new(&exe).exists() {
            if Command::new(&exe).arg(format!("--app={url}")).arg("--window-size=1280,860").spawn().is_ok() {
                return;
            }
        }
    }
    let _ = Command::new("cmd").args(["/C", "start", "", url]).creation_flags(CREATE_NO_WINDOW).spawn();
}

pub fn set_autostart(enabled: bool) -> std::io::Result<()> {
    let status = if enabled {
        let exe = std::env::current_exe()?;
        let value = format!("\"{}\" --background", exe.display());
        Command::new("reg").args(["add", RUN_KEY, "/v", RUN_VALUE, "/t", "REG_SZ", "/d", &value, "/f"]).creation_flags(CREATE_NO_WINDOW).status()?
    } else {
        Command::new("reg").args(["delete", RUN_KEY, "/v", RUN_VALUE, "/f"]).creation_flags(CREATE_NO_WINDOW).status()?
    };
    if status.success() || !enabled {
        Ok(())
    } else {
        Err(std::io::Error::other("failed to update the Windows startup registry key"))
    }
}

/// Lets CLI subcommands print to the terminal even though the release build is a GUI-subsystem exe.
pub fn attach_console() {
    use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}
