//! A single terminal inside a tab.

use std::error::Error;
#[cfg(not(windows))]
use std::os::unix::io::{AsRawFd, RawFd};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::event_loop::EventLoopProxy;
use winit::window::WindowId;

use clauditty_terminal::event::Event as TerminalEvent;
use clauditty_terminal::event_loop::{EventLoop as PtyEventLoop, Msg, Notifier};
use clauditty_terminal::grid::Dimensions;
use clauditty_terminal::index::{Column, Line};
use clauditty_terminal::sync::FairMutex;
use clauditty_terminal::term::Term;
use clauditty_terminal::term::cell::Flags;
use clauditty_terminal::tty;

use crate::config::UiConfig;
#[cfg(not(any(windows, target_os = "openbsd")))]
use crate::daemon::{foreground_process_path, foreground_process_program};
use crate::display::SizeInfo;
use crate::event::{Event, EventProxy, InlineSearchState, SearchState};
use crate::layout::{PaneId, Rect};

/// How often the running program and working directory are looked up.
const INFO_REFRESH_INTERVAL: Duration = Duration::from_millis(500);

/// Programs shown as a plain terminal.
const SHELLS: &[&str] = &["sh", "bash", "zsh", "fish", "dash", "ksh", "tcsh", "csh", "nu", "login"];

/// Program running in a pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneApp {
    Claude,
    Shell,
    Program(String),
}

impl PaneApp {
    /// Name shown in the sidebar.
    pub fn name(&self) -> &str {
        match self {
            PaneApp::Claude => "Claude Code",
            PaneApp::Shell => "Terminal",
            PaneApp::Program(name) => name,
        }
    }

    /// Detect the app from the foreground program and the title it set.
    fn detect(program: Option<&Path>, title: Option<&str>) -> Self {
        let name = program.and_then(Path::file_name).and_then(|name| name.to_str());
        let name = name.map(|name| name.trim_start_matches('-'));

        match name {
            Some(name) if SHELLS.contains(&name) => PaneApp::Shell,
            // Claude Code installs versions as `claude/versions/<version>`.
            _ if program.is_some_and(|path| path.iter().any(|part| part == "claude")) => {
                PaneApp::Claude
            },
            _ if title.is_some_and(|title| title.contains("Claude Code")) => PaneApp::Claude,
            Some(name) => PaneApp::Program(name.to_owned()),
            None => PaneApp::Shell,
        }
    }
}

/// Terminal, shell and per-terminal UI state of one pane.
pub struct Pane {
    pub terminal: Arc<FairMutex<Term<EventProxy>>>,
    pub notifier: Notifier,
    pub search_state: SearchState,
    pub inline_search_state: InlineSearchState,

    /// Terminal size inside the pane.
    pub size_info: SizeInfo,

    /// Area of the pane in window pixels.
    pub rect: Rect,

    /// Title set by the running program.
    pub title: Option<String>,

    /// Foreground program and its working directory, looked up at most every
    /// [`INFO_REFRESH_INTERVAL`].
    program: Option<PathBuf>,
    working_directory: Option<PathBuf>,
    info_updated: Option<Instant>,

    #[cfg(not(windows))]
    pub master_fd: RawFd,
    #[cfg(not(windows))]
    pub shell_pid: u32,
}

impl Pane {
    /// Start a shell in a new pane.
    pub fn new(
        id: PaneId,
        window_id: WindowId,
        proxy: EventLoopProxy<Event>,
        config: &UiConfig,
        pty_config: &tty::Options,
        size_info: SizeInfo,
        rect: Rect,
    ) -> Result<Self, Box<dyn Error>> {
        let event_proxy = EventProxy::new(proxy, window_id).with_pane(id);

        // Create the terminal.
        //
        // This object contains all of the state about what's being displayed. It's
        // wrapped in a clonable mutex since both the I/O loop and display need to
        // access it.
        let terminal = Term::new(config.term_options(), &size_info, event_proxy.clone());
        let terminal = Arc::new(FairMutex::new(terminal));

        // Create the PTY.
        //
        // The PTY forks a process to run the shell on the slave side of the
        // pseudoterminal. A file descriptor for the master side is retained for
        // reading/writing to the shell.
        let pty = tty::new(pty_config, size_info.into(), window_id.into())?;

        #[cfg(not(windows))]
        let master_fd = pty.file().as_raw_fd();
        #[cfg(not(windows))]
        let shell_pid = pty.child().id();

        // Create the pseudoterminal I/O loop.
        //
        // PTY I/O is ran on another thread as to not occupy cycles used by the
        // renderer and input processing. Note that access to the terminal state is
        // synchronized since the I/O loop updates the state, and the display
        // consumes it periodically.
        let event_loop = PtyEventLoop::new(
            Arc::clone(&terminal),
            event_proxy.clone(),
            pty,
            pty_config.drain_on_exit,
            config.debug.ref_test,
        )?;

        // The event loop channel allows write requests from the event processor
        // to be sent to the pty loop and ultimately written to the pty.
        let loop_tx = event_loop.channel();

        // Kick off the I/O thread.
        let _io_thread = event_loop.spawn();

        // Start cursor blinking, in case `Focused` isn't sent on startup.
        if config.cursor.style().blinking {
            event_proxy.send_event(TerminalEvent::CursorBlinkingChange.into());
        }

        Ok(Self {
            terminal,
            notifier: Notifier(loop_tx),
            search_state: Default::default(),
            inline_search_state: Default::default(),
            size_info,
            rect,
            title: None,
            program: None,
            working_directory: None,
            info_updated: None,
            #[cfg(not(windows))]
            master_fd,
            #[cfg(not(windows))]
            shell_pid,
        })
    }

    /// Program running in the pane.
    pub fn app(&mut self) -> PaneApp {
        self.refresh_info();
        PaneApp::detect(self.program.as_deref(), self.title.as_deref())
    }

    /// Working directory of the program running in the pane.
    pub fn working_directory(&mut self) -> Option<&Path> {
        self.refresh_info();
        self.working_directory.as_deref()
    }

    /// Last lines of text on screen, skipping borders and blank lines.
    pub fn preview(&self, max_lines: usize) -> Vec<String> {
        let terminal = self.terminal.lock();
        let grid = terminal.grid();

        let mut lines = Vec::new();
        for line in (0..grid.screen_lines()).rev() {
            let row = &grid[Line(line as i32)];
            let text: String = (0..grid.columns())
                .map(|column| &row[Column(column)])
                .filter(|cell| !cell.flags.contains(Flags::WIDE_CHAR_SPACER))
                .map(|cell| cell.c)
                .collect();

            if let Some(text) = clean_preview_line(&text) {
                lines.push(text);
                if lines.len() == max_lines {
                    break;
                }
            }
        }

        lines.reverse();
        lines
    }

    fn refresh_info(&mut self) {
        if self.info_updated.is_some_and(|updated| updated.elapsed() < INFO_REFRESH_INTERVAL) {
            return;
        }
        self.info_updated = Some(Instant::now());

        #[cfg(not(any(windows, target_os = "openbsd")))]
        {
            self.program = foreground_process_program(self.master_fd, self.shell_pid).ok();
            self.working_directory = foreground_process_path(self.master_fd, self.shell_pid).ok();
        }
    }
}

/// Clean up a screen line for the preview.
///
/// Returns `None` for lines without real content, like borders or an empty prompt.
fn clean_preview_line(line: &str) -> Option<String> {
    // Box drawing and block characters.
    let is_decoration = |c: char| c.is_whitespace() || ('\u{2500}'..='\u{259f}').contains(&c);
    let text = line.trim_matches(is_decoration);

    if text.chars().filter(|c| c.is_alphanumeric()).count() < 2 {
        return None;
    }

    Some(text.split_whitespace().collect::<Vec<_>>().join(" "))
}

impl Drop for Pane {
    fn drop(&mut self) {
        // Shutdown the terminal's PTY.
        let _ = self.notifier.0.send(Msg::Shutdown);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_app() {
        let claude = Path::new("/Users/me/.local/share/claude/versions/2.1.274");
        assert_eq!(PaneApp::detect(Some(claude), None), PaneApp::Claude);
        assert_eq!(PaneApp::detect(Some(Path::new("/opt/node")), Some("✳ Claude Code")), PaneApp::Claude);
        assert_eq!(PaneApp::detect(Some(Path::new("/bin/zsh")), Some("✳ Claude Code")), PaneApp::Shell);
        assert_eq!(PaneApp::detect(Some(Path::new("-zsh")), None), PaneApp::Shell);
        assert_eq!(PaneApp::detect(Some(Path::new("/usr/bin/vim")), None), PaneApp::Program("vim".into()));
        assert_eq!(PaneApp::detect(None, None), PaneApp::Shell);
    }

    #[test]
    fn clean_preview_lines() {
        assert_eq!(clean_preview_line("│ > fix the   login bug   │"), Some("> fix the login bug".into()));
        assert_eq!(clean_preview_line("╰──────────╯"), None);
        assert_eq!(clean_preview_line("  >  "), None);
        assert_eq!(clean_preview_line(""), None);
    }
}
