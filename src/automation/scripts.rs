use serde::{Deserialize, Serialize};

use crate::domain::rooms::Rooms;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Settings {
    pub(crate) safe_start_stop: bool,
    pub(crate) restart_after_reconnect: bool,
    pub(crate) rooms: Rooms,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            safe_start_stop: true,
            restart_after_reconnect: false,
            rooms: Rooms::default(),
        }
    }
}
