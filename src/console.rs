//! Keeping the window open for double-clicked programs.
//!
//! When a console program is started from Explorer — double-clicking a
//! standalone build, or a `.em` file associated with `em` — Windows creates a
//! console just for it and destroys that console the moment the process exits.
//! The output flashes past and the window is gone, which looks exactly like
//! "my program did nothing".
//!
//! Running the same program from a terminal you already had open must *not*
//! pause, or every scripted run would hang waiting for Enter. The two cases
//! are told apart by asking how many processes share this console: if `em` is
//! the only one, the console was created for this launch and dies with it.

use std::io::{self, IsTerminal, Write};

/// Hold a double-click launch open until the user presses Enter.
/// Does nothing when `em` was started from an existing terminal.
pub fn pause_if_window_will_close() {
    if !owns_console() {
        return;
    }
    // Never block when input is redirected (tests, pipes, scripts).
    if !io::stdin().is_terminal() {
        return;
    }
    println!();
    print!("Press Enter to close this window... ");
    let _ = io::stdout().flush();
    let mut discard = String::new();
    let _ = io::stdin().read_line(&mut discard);
}

/// Whether this process is the only one attached to its console, meaning the
/// console was created for it and will be destroyed when it exits.
#[cfg(windows)]
fn owns_console() -> bool {
    use windows_sys::Win32::System::Console::GetConsoleProcessList;

    // A console launched from Explorer has exactly one process in its list:
    // us. Running from cmd, PowerShell or a terminal adds the shell, so the
    // count is at least two. Asking for two is enough to tell those apart.
    let mut processes = [0u32; 2];
    let count = unsafe { GetConsoleProcessList(processes.as_mut_ptr(), processes.len() as u32) };
    count == 1
}

/// Other platforms launch programs from a terminal that outlives them, so
/// there is no vanishing window to guard against.
#[cfg(not(windows))]
fn owns_console() -> bool {
    false
}
