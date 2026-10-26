//! Your own on/off choices about what the window shows, in one place: each is a field here and a
//! row in `SWITCHES`, which Settings draws and the window reads; saved with the rest of what the
//! app remembers, under the same names as before, so an older `app.toml` reads the same.

use serde::{Deserialize, Serialize};

/// The choices, each off (shown) unless set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// A node's detail leaves out its state graph.
    pub hide_state_flow: bool,
    /// The keys beside buttons, rows and menus are left out (the shortcuts list keeps them).
    pub hide_key_hints: bool,
}

/// Where in Settings a switch sits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Appearance,
    Detail,
}

/// A switch in Settings: its name and what it shows, where it sits, and the choice it turns.
pub struct Switch {
    /// Its selector, `settings-<id>-on` or `-off` as it stands.
    pub id: &'static str,
    pub label: &'static str,
    pub what: &'static str,
    pub place: Place,
    /// Whether what it names shows.
    pub shown: fn(&Prefs) -> bool,
    /// Turn it the other way.
    pub flip: fn(&mut Prefs),
}

/// Every switch, in the order Settings lists them.
pub const SWITCHES: [Switch; 2] = [
    Switch {
        id: "state-graph",
        label: "State graph",
        what: "The states it moves through, above its body.",
        place: Place::Detail,
        shown: |p| !p.hide_state_flow,
        flip: |p| p.hide_state_flow = !p.hide_state_flow,
    },
    Switch {
        id: "key-hints",
        label: "Key hints",
        what: "The keys beside buttons, rows and menus. The keyboard shortcuts list always shows \
               them.",
        place: Place::Appearance,
        shown: |p| !p.hide_key_hints,
        flip: |p| p.hide_key_hints = !p.hide_key_hints,
    },
];
