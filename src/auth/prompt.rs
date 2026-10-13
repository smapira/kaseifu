//! Terminal prompts for interactive commands (`setup`, `auth set-key`, and
//! the on-demand key entry in `classify`/`organize`/`watch`).
//!
//! Secret input disables terminal echo and is never logged. All prompts
//! refuse to run without a TTY so scripts get an error instead of a hang.
//! Prompts are written to **stderr**: stdout stays parseable when it is
//! redirected or piped (`--format json`), and the prompt is still visible
//! when stdout is not a terminal.

use anyhow::{Context, Result};
use std::io::{IsTerminal, Write};

/// Read one line after showing `prompt` (echo kept, for non-secret values).
pub fn prompt_line(prompt: &str, default: Option<&str>) -> Result<String> {
    require_tty()?;
    show_prompt(prompt, default)?;
    read_line_trimmed()
}

/// Read a secret with terminal echo disabled (TTY only, never logged).
pub fn prompt_secret(prompt: &str) -> Result<String> {
    require_tty()?;
    show_prompt(prompt, None)?;
    read_secret_trimmed()
}

/// Ask a yes/no question; empty input resolves to `default_yes`.
pub fn confirm(prompt: &str, default_yes: bool) -> Result<bool> {
    require_tty()?;
    let hint = if default_yes { "Y/n" } else { "y/N" };
    show_prompt(&format!("{prompt} [{hint}]"), None)?;
    parse_confirmation(&read_line_trimmed()?, default_yes)
}

fn require_tty() -> Result<()> {
    if std::io::stdin().is_terminal() {
        Ok(())
    } else {
        anyhow::bail!("interactive input needs a terminal (stdin is not a TTY)")
    }
}

fn show_prompt(prompt: &str, default: Option<&str>) -> Result<()> {
    // stderr, not stdout: the answer is the only thing a caller may parse,
    // and a redirected stdout must not swallow the prompt (which would look
    // like a hang).
    let mut stderr = std::io::stderr();
    match default {
        Some(current) => write!(stderr, "{prompt} [{current}]: "),
        None => write!(stderr, "{prompt}: "),
    }
    .context("write prompt to terminal")?;
    stderr.flush().context("flush prompt to terminal")?;
    Ok(())
}

fn read_line_trimmed() -> Result<String> {
    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .context("read input from terminal")?;
    Ok(input.trim().to_string())
}

fn parse_confirmation(input: &str, default_yes: bool) -> Result<bool> {
    match input.to_ascii_lowercase().as_str() {
        "" => Ok(default_yes),
        "y" | "yes" => Ok(true),
        "n" | "no" => Ok(false),
        _ => anyhow::bail!("expected y or n"),
    }
}

#[cfg(unix)]
fn read_secret_trimmed() -> Result<String> {
    let guard = EchoGuard::disable().context("disable terminal echo")?;
    let mut input = String::new();
    let read = std::io::stdin().read_line(&mut input);
    drop(guard);
    eprintln!();
    read.context("read secret from terminal")?;
    Ok(input.trim().to_string())
}

#[cfg(not(unix))]
fn read_secret_trimmed() -> Result<String> {
    anyhow::bail!("secret input is only supported on Unix terminals")
}

/// Restores terminal echo on drop (interrupts cannot leak a muted terminal).
#[cfg(unix)]
struct EchoGuard {
    fd: std::os::fd::RawFd,
    original: libc::termios,
}

#[cfg(unix)]
impl EchoGuard {
    fn disable() -> Result<Self> {
        use std::os::fd::AsRawFd;
        let fd = std::io::stdin().as_raw_fd();
        let original = current_termios(fd)?;
        let mut muted = original;
        muted.c_lflag &= !libc::ECHO;
        set_termios(fd, &muted)?;
        Ok(Self { fd, original })
    }
}

#[cfg(unix)]
impl Drop for EchoGuard {
    fn drop(&mut self) {
        let _ = set_termios(self.fd, &self.original);
    }
}

#[cfg(unix)]
fn current_termios(fd: std::os::fd::RawFd) -> Result<libc::termios> {
    let mut termios: libc::termios = unsafe { std::mem::zeroed() };
    if unsafe { libc::tcgetattr(fd, &mut termios) } != 0 {
        anyhow::bail!("read terminal attributes");
    }
    Ok(termios)
}

#[cfg(unix)]
fn set_termios(fd: std::os::fd::RawFd, termios: &libc::termios) -> Result<()> {
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, termios) } != 0 {
        anyhow::bail!("update terminal attributes");
    }
    Ok(())
}
