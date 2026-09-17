use serde::Serialize;

use clauditty_config_derive::ConfigDeserialize;

#[derive(ConfigDeserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Tabs {
    /// Command run in the main pane of each new tab.
    ///
    /// An empty command starts a plain shell.
    pub command: String,
}

impl Default for Tabs {
    fn default() -> Self {
        Self { command: String::from("claude") }
    }
}
