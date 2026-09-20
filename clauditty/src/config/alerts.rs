use serde::Serialize;

use clauditty_config_derive::ConfigDeserialize;

#[derive(ConfigDeserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Alerts {
    /// Show the number of ready tabs on the Dock icon.
    pub badge: bool,

    /// Bounce the Dock icon when a tab becomes ready while the window is away.
    pub bounce: bool,
}

impl Default for Alerts {
    fn default() -> Self {
        Self { badge: true, bounce: true }
    }
}
