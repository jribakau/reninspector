//! Helper processes the IDE starts and reads (`git`, `tasklist`, SDK commands).

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

use crate::error::AppError;

/// A command that runs out of sight. A release build has no console of its own,
/// so on Windows a console program would otherwise flash a window on every call.
pub fn background(program: impl AsRef<OsStr>) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

/// Show `path` in the system file manager. A file is selected; a folder is opened.
pub fn reveal(path: &Path) -> Result<(), AppError> {
    let mut cmd = if cfg!(windows) {
        let mut cmd = background("explorer");
        let native = path.to_string_lossy().replace('/', "\\");
        if path.is_dir() {
            cmd.arg(native);
        } else {
            cmd.arg(format!("/select,{native}"));
        }
        cmd
    } else if cfg!(target_os = "macos") {
        let mut cmd = background("open");
        if path.is_dir() {
            cmd.arg(path);
        } else {
            cmd.arg("-R").arg(path);
        }
        cmd
    } else {
        let mut cmd = background("xdg-open");
        cmd.arg(if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        });
        cmd
    };
    cmd.spawn()
        .map_err(|e| AppError::new(format!("Could not open the file manager: {e}")))?;
    Ok(())
}
