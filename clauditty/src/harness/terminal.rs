//! A plain shell, or any program without its own harness.

use crate::display::color::Rgb;
use crate::harness::{ActivityRule, Harness, PaneProcess};

pub struct Terminal;

impl Harness for Terminal {
    fn detect(&self, _process: &PaneProcess<'_>) -> bool {
        true
    }

    fn title(&self, process: &PaneProcess<'_>) -> String {
        match process.program_name() {
            Some(name) if !process.shell_in_front() => name.to_owned(),
            _ => String::from("Terminal"),
        }
    }

    fn icon(&self) -> char {
        '›'
    }

    fn accent_color(&self) -> Rgb {
        Rgb::new(0xd0, 0xd7, 0xde)
    }

    fn activity_rule(&self) -> ActivityRule {
        ActivityRule::ForegroundProgram
    }
}
