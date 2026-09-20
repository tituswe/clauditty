//! Saving and restoring the tabs and panes of a window.

use std::fs;
use std::path::PathBuf;

use log::{debug, warn};
use serde::{Deserialize, Serialize};

use crate::layout::{Layout, PaneId};

/// Everything restored when Clauditty starts again.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct Session {
    pub tabs: Vec<TabSession>,
    pub active_tab: usize,
}

/// One saved tab, with its split layout.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TabSession {
    pub layout: Layout,
    pub focused: PaneId,
    pub panes: Vec<(PaneId, PaneSession)>,
}

/// One saved pane.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PaneSession {
    /// Command reopening the pane's program, or `None` for a plain shell.
    pub command: Option<String>,
    pub working_directory: Option<PathBuf>,
}

/// File the session is stored in.
#[cfg(not(windows))]
fn path() -> Option<PathBuf> {
    xdg::BaseDirectories::with_prefix("clauditty").place_state_file("session.json").ok()
}

#[cfg(windows)]
fn path() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("clauditty").join("session.json"))
}

/// Read the session saved by the last run.
pub fn load() -> Option<Session> {
    let path = path()?;
    let contents = fs::read_to_string(&path).ok()?;

    match serde_json::from_str(&contents) {
        Ok(session) => Some(session),
        Err(err) => {
            warn!("Ignoring session in {path:?}: {err}");
            None
        },
    }
}

/// Store the session for the next run.
pub fn save(session: &Session) {
    let Some(path) = path() else { return };

    let result = serde_json::to_string(session)
        .map_err(|err| err.to_string())
        .and_then(|contents| fs::write(&path, contents).map_err(|err| err.to_string()));

    match result {
        Ok(()) => debug!("Saved session to {path:?}"),
        Err(err) => warn!("Could not save session: {err}"),
    }
}
