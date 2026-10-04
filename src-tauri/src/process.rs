//! Helper processes the IDE starts and reads (`git`, `tasklist`, SDK commands).

use std::ffi::OsStr;
use std::process::Command;

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
