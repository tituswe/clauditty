//! Claude Code, Anthropic's coding agent.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

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

    fn restore_command(
        &self,
        working_directory: Option<&Path>,
        taken: &mut HashSet<PathBuf>,
    ) -> Option<String> {
        match working_directory.and_then(|directory| newest_session(directory, taken)) {
            Some(session) => Some(format!("claude --resume {session}")),
            None => Some(String::from("claude")),
        }
    }
}

/// Newest conversation Claude Code saved for a directory, skipping claimed ones.
fn newest_session(working_directory: &Path, taken: &mut HashSet<PathBuf>) -> Option<String> {
    // Claude Code keeps conversations in `~/.claude/projects/<path with dashes>`.
    let slug = working_directory.to_str()?.replace(['/', '.'], "-");
    let directory = home::home_dir()?.join(".claude/projects").join(slug);

    let mut sessions: Vec<_> = fs::read_dir(directory)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "jsonl"))
        .filter(|path| !taken.contains(path))
        .filter_map(|path| Some((path.metadata().ok()?.modified().ok()?, path)))
        .collect();
    sessions.sort_by_key(|(modified, _)| *modified);

    let (_, path) = sessions.pop()?;
    let session = path.file_stem()?.to_str()?.to_owned();
    taken.insert(path);

    Some(session)
}
