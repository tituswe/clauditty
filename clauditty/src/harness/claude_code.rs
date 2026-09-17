//! Claude Code, Anthropic's coding agent.

use crate::display::color::Rgb;
use crate::harness::{ActivityRule, Harness, PaneProcess};

pub struct ClaudeCode;

impl Harness for ClaudeCode {
    fn detect(&self, process: &PaneProcess<'_>) -> bool {
        // Native installs run from `claude/versions/<version>`. Other installs run under Node,
        // but set the title.
        let native = process.program.is_some_and(|path| path.iter().any(|part| part == "claude"));
        native || process.title.is_some_and(|title| title.contains("Claude Code"))
    }

    fn title(&self, _process: &PaneProcess<'_>) -> String {
        String::from("Claude Code")
    }

    fn icon(&self) -> char {
        '✳'
    }

    fn accent_color(&self) -> Rgb {
        Rgb::new(0xd9, 0x77, 0x57)
    }

    fn activity_rule(&self) -> ActivityRule {
        // The spinner redraws constantly while Claude Code works.
        ActivityRule::OutputQuiet
    }
}
