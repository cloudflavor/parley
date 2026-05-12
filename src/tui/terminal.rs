use anyhow::{Context, Result, bail};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io::{self, IsTerminal};

pub(super) struct TerminalSession {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
    mouse_capture_enabled: bool,
    restored: bool,
}

impl TerminalSession {
    pub(super) fn new(mouse_capture_enabled: bool) -> Result<Self> {
        ensure_terminal_io()?;

        enable_raw_mode().context("failed to enable raw mode")?;
        let mut stdout = io::stdout();
        if mouse_capture_enabled {
            execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
                .context("failed to enter alternate screen")?;
        } else {
            execute!(stdout, EnterAlternateScreen).context("failed to enter alternate screen")?;
        }

        let terminal = Terminal::new(CrosstermBackend::new(stdout))
            .context("failed to initialize terminal")?;

        Ok(Self {
            terminal,
            mouse_capture_enabled,
            restored: false,
        })
    }

    pub(super) fn terminal_mut(&mut self) -> &mut Terminal<CrosstermBackend<io::Stdout>> {
        &mut self.terminal
    }

    pub(super) fn mouse_capture_enabled(&self) -> bool {
        self.mouse_capture_enabled
    }

    fn restore(&mut self) -> Result<()> {
        if self.restored {
            return Ok(());
        }

        disable_raw_mode().context("failed to disable raw mode")?;
        if self.mouse_capture_enabled {
            execute!(
                self.terminal.backend_mut(),
                LeaveAlternateScreen,
                DisableMouseCapture
            )
            .context("failed to leave alternate screen")?;
        } else {
            execute!(self.terminal.backend_mut(), LeaveAlternateScreen)
                .context("failed to leave alternate screen")?;
        }
        self.terminal
            .show_cursor()
            .context("failed to show cursor")?;
        self.restored = true;

        Ok(())
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        if let Err(error) = self.restore() {
            eprintln!("parley: failed to restore terminal state: {error}");
        }
    }
}

fn ensure_terminal_io() -> Result<()> {
    validate_terminal_io(io::stdin().is_terminal(), io::stdout().is_terminal())
}

fn validate_terminal_io(stdin_is_terminal: bool, stdout_is_terminal: bool) -> Result<()> {
    if !stdin_is_terminal {
        bail!(
            "parley tui requires interactive stdin; run it in a real terminal or allocate a PTY with `ssh -t`"
        );
    }

    if !stdout_is_terminal {
        bail!(
            "parley tui requires interactive stdout; run it in a real terminal instead of piping output"
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_terminal_io;

    #[test]
    fn validate_terminal_io_accepts_interactive_streams() {
        assert!(validate_terminal_io(true, true).is_ok());
    }

    #[test]
    fn validate_terminal_io_rejects_non_terminal_stdin() {
        let error = validate_terminal_io(false, true).expect_err("stdin should require a tty");
        assert!(
            error.to_string().contains("requires interactive stdin"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn validate_terminal_io_rejects_non_terminal_stdout() {
        let error = validate_terminal_io(true, false).expect_err("stdout should require a tty");
        assert!(
            error.to_string().contains("requires interactive stdout"),
            "unexpected error: {error}"
        );
    }
}
