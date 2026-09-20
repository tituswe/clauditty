//! Harnesses: the kinds of programs Clauditty knows how to show, like Claude Code.
//!
//! A harness decides how a pane is detected, named, colored, previewed and tracked. To support a
//! new agent, add a type implementing [`Harness`] and list it in [`AGENTS`].

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::display::color::Rgb;

mod claude_code;
mod terminal;

pub use claude_code::ClaudeCode;
pub use terminal::Terminal;

/// Agent harnesses, checked in order. Panes matching none of them are shown as a [`Terminal`].
static AGENTS: &[&dyn Harness] = &[&ClaudeCode];

/// Programs treated as a plain shell.
const SHELLS: &[&str] = &["sh", "bash", "zsh", "fish", "dash", "ksh", "tcsh", "csh", "nu", "login"];

/// How a harness tells that it's working.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityRule {
    /// Working while printing, finished once quiet. Suits agents with a live spinner.
    OutputQuiet,
    /// Working while a program other than the shell is in front.
    ForegroundProgram,
}

/// What a pane is running, as seen from outside the terminal.
#[derive(Debug, Default, Clone, Copy)]
pub struct PaneProcess<'a> {
    /// Executable of the program in front.
    pub program: Option<&'a Path>,
    /// Title set by the running program.
    pub title: Option<&'a str>,
}

impl PaneProcess<'_> {
    /// File name of the program in front, without the leading `-` of login shells.
    pub fn program_name(&self) -> Option<&str> {
        let name = self.program?.file_name()?.to_str()?;
        Some(name.trim_start_matches('-'))
    }

    /// Whether the shell itself is in front, so no program is running.
    pub fn shell_in_front(&self) -> bool {
        self.program_name().is_none_or(|name| SHELLS.contains(&name))
    }
}

/// How a kind of program is detected and shown.
pub trait Harness: Sync {
    /// Whether this harness is running in a pane.
    fn detect(&self, process: &PaneProcess<'_>) -> bool;

    /// Name shown on the tab.
    fn title(&self, process: &PaneProcess<'_>) -> String;

    /// Icon shown before the title.
    fn icon(&self) -> char;

    /// Color of the icon, the active tab marker and the focused pane border.
    fn accent_color(&self) -> Rgb;

    /// How to tell when the harness is working.
    fn activity_rule(&self) -> ActivityRule;

    /// Command reopening the harness where it left off, or `None` for a plain shell.
    ///
    /// `taken` holds the saved sessions already claimed by other panes.
    fn restore_command(
        &self,
        _working_directory: Option<&Path>,
        _taken: &mut HashSet<PathBuf>,
    ) -> Option<String> {
        None
    }

    /// Lines to preview on the tab, picked from the lines on screen from top to bottom.
    fn preview(&self, screen: &[String], max_lines: usize) -> Vec<String> {
        last_lines(screen, max_lines)
    }
}

/// Find the harness running in a pane.
pub fn detect(process: &PaneProcess<'_>) -> &'static dyn Harness {
    // With the shell in front, any agent has already exited.
    if process.shell_in_front() {
        return &Terminal;
    }

    AGENTS.iter().copied().find(|agent| agent.detect(process)).unwrap_or(&Terminal)
}

/// Last lines with real content, skipping borders and blank lines.
pub fn last_lines(screen: &[String], max_lines: usize) -> Vec<String> {
    let mut lines: Vec<_> =
        screen.iter().rev().filter_map(|line| clean_line(line)).take(max_lines).collect();
    lines.reverse();
    lines
}

/// Trim borders and extra spaces from a line, or `None` if nothing is left.
fn clean_line(line: &str) -> Option<String> {
    // Box drawing and block characters.
    let is_decoration = |c: char| c.is_whitespace() || ('\u{2500}'..='\u{259f}').contains(&c);
    let text = line.trim_matches(is_decoration);

    if text.chars().filter(|c| c.is_alphanumeric()).count() < 2 {
        return None;
    }

    Some(text.split_whitespace().collect::<Vec<_>>().join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process<'a>(program: Option<&'a str>, title: Option<&'a str>) -> PaneProcess<'a> {
        PaneProcess { program: program.map(Path::new), title }
    }

    fn title(process: &PaneProcess<'_>) -> String {
        detect(process).title(process)
    }

    #[test]
    fn detect_harness() {
        let native = process(Some("/Users/me/.local/share/claude/versions/2.1.274"), None);
        assert_eq!(title(&native), "Claude Code");
        assert_eq!(title(&process(Some("/opt/node"), Some("✳ Claude Code"))), "Claude Code");

        // Claude Code clears its title on exit, but may leave a stale one behind.
        assert_eq!(title(&process(Some("/bin/zsh"), Some("✳ Claude Code"))), "Terminal");
        assert_eq!(title(&process(Some("-zsh"), None)), "Terminal");
        assert_eq!(title(&process(None, None)), "Terminal");
        assert_eq!(title(&process(Some("/usr/bin/vim"), None)), "vim");
    }

    #[test]
    fn preview_lines() {
        let screen: Vec<String> = [
            "╭──────────╮",
            "│ > fix the   login bug   │",
            "╰──────────╯",
            "  >  ",
            "",
            "done",
        ]
        .iter()
        .map(|line| line.to_string())
        .collect();

        assert_eq!(last_lines(&screen, 2), vec!["> fix the login bug", "done"]);
        assert_eq!(last_lines(&screen, 1), vec!["done"]);
    }
}
